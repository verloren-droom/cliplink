use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use crate::{
    constants::{app::LOCAL_HOSTNAME, timing::RUNTIME_DISCOVERY_INTERVAL},
    core::{
        at_rest::LocalDataCipher,
        config::AppConfig,
        discovery::{discover_once, start_advertiser},
        error::{AppError, AppResult},
        model::{ClipboardItem, ClipboardPayload, DiscoveredPeer},
        paths::AppPaths,
        protocol::TransferHeader,
        security::{
            DeviceIdentity, build_rustls_client_config, build_rustls_server_config,
            load_or_create_identity,
        },
    },
};
use crossbeam_channel::{Receiver, Sender};
use parking_lot::RwLock;
use quinn::{
    ClientConfig, Connection, Endpoint, ServerConfig,
    crypto::rustls::{QuicClientConfig, QuicServerConfig},
};
use tokio::{
    fs::{self, File},
    io::{AsyncReadExt, AsyncWriteExt},
    runtime::Builder,
    sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
    time::{MissedTickBehavior, interval},
};

const MAX_TRANSFER_HEADER_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone)]
pub enum RuntimeCommand {
    Broadcast(ClipboardItem),
    ReplaceConfig(AppConfig),
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    PeerList(Vec<DiscoveredPeer>),
    InboundClipboard(ClipboardItem),
    Status(String),
}

#[derive(Debug)]
pub struct RuntimeBridge {
    command_tx: UnboundedSender<RuntimeCommand>,
    event_rx: Receiver<RuntimeEvent>,
}

#[derive(Default)]
struct RuntimeResources {
    identity: Option<DeviceIdentity>,
    endpoint: Option<Endpoint>,
    endpoint_port: Option<u16>,
    advertiser: Option<agnostic_mdns::tokio::Server>,
    advertiser_key: Option<AdvertiserKey>,
}

struct RuntimeShared {
    local_data_cipher: LocalDataCipher,
    trusted_fingerprints: Arc<RwLock<HashSet<String>>>,
    inbox_dir: Arc<RwLock<PathBuf>>,
    event_tx: Sender<RuntimeEvent>,
    shutdown: Arc<AtomicBool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AdvertiserKey {
    device_name: String,
    port: u16,
    fingerprint: String,
}

impl RuntimeBridge {
    pub fn start(
        paths: AppPaths,
        config: AppConfig,
        local_data_cipher: LocalDataCipher,
    ) -> AppResult<Self> {
        let (command_tx, command_rx) = unbounded_channel();
        let (event_tx, event_rx) = crossbeam_channel::unbounded();

        thread::Builder::new()
            .name("cliplink-runtime".to_string())
            .stack_size(512 * 1024)
            .spawn(move || {
                let runtime = Builder::new_current_thread()
                    .enable_all()
                    .max_blocking_threads(2)
                    .build();

                match runtime {
                    Ok(runtime) => {
                        if let Err(error) = runtime.block_on(run_runtime(
                            paths,
                            config,
                            local_data_cipher,
                            command_rx,
                            event_tx.clone(),
                        )) {
                            let _ = event_tx
                                .send(RuntimeEvent::Status(format!("后台运行时失败: {error}")));
                        }
                    }
                    Err(error) => {
                        let _ = event_tx.send(RuntimeEvent::Status(format!(
                            "无法创建 Tokio 运行时: {error}"
                        )));
                    }
                }
            })
            .map_err(AppError::from)?;

        Ok(Self {
            command_tx,
            event_rx,
        })
    }

    pub fn send(&self, command: RuntimeCommand) {
        let _ = self.command_tx.send(command);
    }

    pub fn try_recv(&self) -> Option<RuntimeEvent> {
        self.event_rx.try_recv().ok()
    }
}

impl Drop for RuntimeBridge {
    fn drop(&mut self) {
        let _ = self.command_tx.send(RuntimeCommand::Shutdown);
    }
}

async fn run_runtime(
    paths: AppPaths,
    config: AppConfig,
    local_data_cipher: LocalDataCipher,
    mut command_rx: UnboundedReceiver<RuntimeCommand>,
    event_tx: Sender<RuntimeEvent>,
) -> AppResult<()> {
    let shutdown = Arc::new(AtomicBool::new(false));
    let config_state = Arc::new(RwLock::new(config));
    let discovered = Arc::new(RwLock::new(HashMap::<String, DiscoveredPeer>::new()));
    let mut resources = RuntimeResources::default();
    let shared = RuntimeShared {
        local_data_cipher,
        trusted_fingerprints: Arc::new(RwLock::new(trusted_fingerprint_set(&config_state.read()))),
        inbox_dir: Arc::new(RwLock::new(paths.inbox_dir.clone())),
        event_tx: event_tx.clone(),
        shutdown: shutdown.clone(),
    };
    std::fs::create_dir_all(shared.inbox_dir.read().as_path())?;

    let initial_config = config_state.read().clone();
    reconcile_runtime_resources(&paths, &initial_config, &shared, &mut resources).await?;

    spawn_discovery_loop(
        config_state.clone(),
        discovered.clone(),
        event_tx.clone(),
        shutdown.clone(),
    );

    while let Some(command) = command_rx.recv().await {
        match command {
            RuntimeCommand::Broadcast(item) => {
                if !config_state.read().auto_sync || !config_state.read().share_local_history {
                    continue;
                }
                let Some(endpoint) = resources.endpoint.as_ref().cloned() else {
                    continue;
                };

                let peers = discovered
                    .read()
                    .values()
                    .filter(|peer| {
                        shared
                            .trusted_fingerprints
                            .read()
                            .contains(&peer.fingerprint)
                    })
                    .cloned()
                    .collect::<Vec<_>>();

                for peer in peers {
                    let endpoint = endpoint.clone();
                    let event_tx = shared.event_tx.clone();
                    let item = item.clone();
                    tokio::spawn(async move {
                        if let Err(error) = send_to_peer(endpoint, peer.clone(), item).await {
                            let _ = event_tx.send(RuntimeEvent::Status(format!(
                                "同步到 {} 失败: {}",
                                peer.device_name, error
                            )));
                        }
                    });
                }
            }
            RuntimeCommand::ReplaceConfig(new_config) => {
                let old_port = config_state.read().listen_port;
                *config_state.write() = new_config.clone();
                *shared.trusted_fingerprints.write() = trusted_fingerprint_set(&new_config);
                *shared.inbox_dir.write() = paths.inbox_dir.clone();
                if let Err(error) =
                    reconcile_runtime_resources(&paths, &new_config, &shared, &mut resources).await
                {
                    let _ = shared.event_tx.send(RuntimeEvent::Status(format!(
                        "后台网络服务更新失败: {error}"
                    )));
                } else if old_port != new_config.listen_port {
                    let _ = shared
                        .event_tx
                        .send(RuntimeEvent::Status("监听端口已更新".to_string()));
                }
            }
            RuntimeCommand::Shutdown => {
                shutdown.store(true, Ordering::Relaxed);
                resources.shutdown();
                break;
            }
        }
    }

    if let Some(endpoint) = resources.endpoint.take() {
        endpoint.wait_idle().await;
    }
    Ok(())
}

fn spawn_accept_loop(
    endpoint: Endpoint,
    inbox_dir: Arc<RwLock<PathBuf>>,
    event_tx: Sender<RuntimeEvent>,
    shutdown: Arc<AtomicBool>,
) {
    tokio::spawn(async move {
        while !shutdown.load(Ordering::Relaxed) {
            let Some(incoming) = endpoint.accept().await else {
                break;
            };

            let event_tx = event_tx.clone();
            let inbox_dir = inbox_dir.read().clone();
            tokio::spawn(async move {
                if let Err(error) =
                    handle_connection(incoming.await, inbox_dir, event_tx.clone()).await
                {
                    let _ = event_tx.send(RuntimeEvent::Status(format!("接收连接失败: {error}")));
                }
            });
        }
    });
}

fn spawn_discovery_loop(
    config_state: Arc<RwLock<AppConfig>>,
    discovered: Arc<RwLock<HashMap<String, DiscoveredPeer>>>,
    event_tx: Sender<RuntimeEvent>,
    shutdown: Arc<AtomicBool>,
) {
    tokio::spawn(async move {
        let mut last_signature = peer_list_signature(&[]);
        let mut ticker = interval(RUNTIME_DISCOVERY_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

        while !shutdown.load(Ordering::Relaxed) {
            if config_state.read().discovery_enabled {
                let device_id = config_state.read().device_id.clone();
                match discover_once(&device_id).await {
                    Ok(peers) => {
                        let signature = peer_list_signature(&peers);
                        {
                            let mut map = discovered.write();
                            map.clear();
                            for peer in &peers {
                                map.insert(peer.device_id.clone(), peer.clone());
                            }
                        }
                        if signature != last_signature {
                            last_signature = signature;
                            let _ = event_tx.send(RuntimeEvent::PeerList(peers));
                        }
                    }
                    Err(error) => {
                        let _ =
                            event_tx.send(RuntimeEvent::Status(format!("设备发现失败: {error}")));
                    }
                }
            } else if !last_signature.is_empty() {
                discovered.write().clear();
                last_signature.clear();
                let _ = event_tx.send(RuntimeEvent::PeerList(Vec::new()));
            }

            ticker.tick().await;
        }
    });
}

async fn handle_connection(
    connection: Result<Connection, quinn::ConnectionError>,
    inbox_dir: PathBuf,
    event_tx: Sender<RuntimeEvent>,
) -> AppResult<()> {
    let connection = connection.map_err(|error| AppError::Network(error.to_string()))?;
    loop {
        let stream = connection.accept_bi().await;
        let (mut send, mut recv) = match stream {
            Ok(stream) => stream,
            Err(quinn::ConnectionError::ApplicationClosed { .. }) => return Ok(()),
            Err(error) => return Err(AppError::Network(error.to_string())),
        };

        let item = receive_item(&mut recv, &inbox_dir).await?;
        let _ = event_tx.send(RuntimeEvent::InboundClipboard(item));
        send.finish()
            .map_err(|error| AppError::Network(error.to_string()))?;
    }
}

async fn receive_item(
    recv: &mut quinn::RecvStream,
    inbox_dir: &std::path::Path,
) -> AppResult<ClipboardItem> {
    let mut len_buf = [0_u8; 4];
    recv.read_exact(&mut len_buf)
        .await
        .map_err(|error| AppError::Network(error.to_string()))?;
    let header_len = u32::from_be_bytes(len_buf) as usize;
    if header_len == 0 || header_len > MAX_TRANSFER_HEADER_BYTES {
        return Err(AppError::Network(format!(
            "Transfer header length {header_len} is invalid"
        )));
    }
    let mut header_bytes = vec![0_u8; header_len];
    recv.read_exact(&mut header_bytes)
        .await
        .map_err(|error| AppError::Network(error.to_string()))?;

    let header: TransferHeader = serde_json::from_slice(&header_bytes)?;
    let mut local_files = Vec::new();
    if let crate::core::model::ClipboardKind::Files = header.item.kind {
        let base_dir = inbox_dir.join(header.item.id.to_string());
        fs::create_dir_all(&base_dir).await?;

        for file in &header.item.files {
            let safe_relative = crate::core::clipboard::sanitize_relative_path(&file.relative_path);
            let output_path = base_dir.join(safe_relative);
            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent).await?;
            }

            let mut output = File::create(&output_path).await?;
            let mut remaining = file.size_bytes;
            let mut buffer = vec![0_u8; 64 * 1024];
            while remaining > 0 {
                let to_read = remaining.min(buffer.len() as u64) as usize;
                recv.read_exact(&mut buffer[..to_read])
                    .await
                    .map_err(|error| AppError::Network(error.to_string()))?;
                output.write_all(&buffer[..to_read]).await?;
                remaining -= to_read as u64;
            }
            local_files.push(output_path);
        }
    }

    Ok(header.into_item(local_files))
}

async fn send_to_peer(
    endpoint: Endpoint,
    peer: DiscoveredPeer,
    item: ClipboardItem,
) -> AppResult<()> {
    let address = parse_peer_addr(&peer)?;
    let connection = endpoint
        .connect(address, LOCAL_HOSTNAME)
        .map_err(|error| AppError::Network(error.to_string()))?
        .await
        .map_err(|error| AppError::Network(error.to_string()))?;
    let (mut send, _recv) = connection
        .open_bi()
        .await
        .map_err(|error| AppError::Network(error.to_string()))?;
    write_item(&mut send, &item).await?;
    send.finish()
        .map_err(|error| AppError::Network(error.to_string()))?;
    Ok(())
}

async fn write_item(send: &mut quinn::SendStream, item: &ClipboardItem) -> AppResult<()> {
    let header = TransferHeader::from_item(item);
    let header_bytes = serde_json::to_vec(&header)?;
    send.write_all(&(header_bytes.len() as u32).to_be_bytes())
        .await
        .map_err(|error| AppError::Network(error.to_string()))?;
    send.write_all(&header_bytes)
        .await
        .map_err(|error| AppError::Network(error.to_string()))?;

    if let ClipboardPayload::Files(files) = &item.payload {
        let mut buffer = vec![0_u8; 64 * 1024];
        for file in files {
            let source_path = file
                .source_path
                .as_ref()
                .or(file.local_path.as_ref())
                .ok_or_else(|| {
                    AppError::InvalidConfig("File entry is missing a source path".to_string())
                })?;
            let mut input = File::open(source_path).await?;
            loop {
                let bytes_read = input.read(&mut buffer).await?;
                if bytes_read == 0 {
                    break;
                }
                send.write_all(&buffer[..bytes_read])
                    .await
                    .map_err(|error| AppError::Network(error.to_string()))?;
            }
        }
    }

    Ok(())
}

fn build_endpoint(
    port: u16,
    identity: &DeviceIdentity,
    trusted_fingerprints: Arc<RwLock<HashSet<String>>>,
) -> AppResult<Endpoint> {
    let server_crypto = build_rustls_server_config(identity, trusted_fingerprints.clone())?;
    let client_crypto = build_rustls_client_config(identity, trusted_fingerprints)?;

    let mut server_config = ServerConfig::with_crypto(Arc::new(
        QuicServerConfig::try_from(server_crypto)
            .map_err(|error| AppError::Crypto(error.to_string()))?,
    ));
    let transport = Arc::get_mut(&mut server_config.transport).ok_or_else(|| {
        AppError::Network("QUIC transport configuration is not available".to_string())
    })?;
    transport.max_concurrent_uni_streams(0_u8.into());

    let mut endpoint = Endpoint::server(
        server_config,
        SocketAddr::new(IpAddr::from([0, 0, 0, 0]), port),
    )
    .map_err(|error| AppError::Network(error.to_string()))?;
    endpoint.set_default_client_config(ClientConfig::new(Arc::new(
        QuicClientConfig::try_from(client_crypto)
            .map_err(|error| AppError::Crypto(error.to_string()))?,
    )));
    Ok(endpoint)
}

fn parse_peer_addr(peer: &DiscoveredPeer) -> AppResult<SocketAddr> {
    let ip = peer
        .address
        .parse::<IpAddr>()
        .map_err(|error| AppError::Network(error.to_string()))?;
    Ok(SocketAddr::new(ip, peer.port))
}

fn trusted_fingerprint_set(config: &AppConfig) -> HashSet<String> {
    config
        .trusted_peers
        .iter()
        .map(|peer| peer.fingerprint.clone())
        .collect()
}

fn transfer_service_enabled(config: &AppConfig) -> bool {
    !config.trusted_peers.is_empty()
}

fn advertiser_enabled(config: &AppConfig) -> bool {
    config.discovery_enabled
}

async fn reconcile_runtime_resources(
    paths: &AppPaths,
    config: &AppConfig,
    shared: &RuntimeShared,
    resources: &mut RuntimeResources,
) -> AppResult<()> {
    let advertiser_requested = advertiser_enabled(config);
    let transfer_requested = transfer_service_enabled(config);

    if !advertiser_requested && !transfer_requested {
        resources.shutdown();
        return Ok(());
    }

    if resources.identity.is_none() {
        resources.identity = Some(load_or_create_identity(
            paths,
            &config.device_id,
            &shared.local_data_cipher,
        )?);
    }
    let identity = resources
        .identity
        .as_ref()
        .expect("runtime identity must be initialized");

    let advertiser_key = AdvertiserKey {
        device_name: config.device_name.clone(),
        port: config.listen_port,
        fingerprint: identity.fingerprint.clone(),
    };

    if advertiser_requested {
        let needs_restart = resources.advertiser.is_none()
            || resources.advertiser_key.as_ref() != Some(&advertiser_key);
        if needs_restart {
            resources.advertiser.take();
            match start_advertiser(config, identity).await {
                Ok(server) => {
                    resources.advertiser = Some(server);
                    resources.advertiser_key = Some(advertiser_key);
                }
                Err(error) => {
                    let _ = shared
                        .event_tx
                        .send(RuntimeEvent::Status(format!("mDNS 广播启动失败: {error}")));
                }
            }
        }
    } else {
        resources.advertiser.take();
        resources.advertiser_key = None;
    }

    if transfer_requested {
        let needs_restart =
            resources.endpoint.is_none() || resources.endpoint_port != Some(config.listen_port);
        if needs_restart {
            if let Some(endpoint) = resources.endpoint.take() {
                endpoint.close(0u32.into(), b"reconfigure");
            }

            let endpoint = build_endpoint(
                config.listen_port,
                identity,
                shared.trusted_fingerprints.clone(),
            )?;
            spawn_accept_loop(
                endpoint.clone(),
                shared.inbox_dir.clone(),
                shared.event_tx.clone(),
                shared.shutdown.clone(),
            );
            resources.endpoint = Some(endpoint);
            resources.endpoint_port = Some(config.listen_port);
        }
    } else if let Some(endpoint) = resources.endpoint.take() {
        endpoint.close(0u32.into(), b"disabled");
        resources.endpoint_port = None;
    }

    Ok(())
}

fn peer_list_signature(peers: &[DiscoveredPeer]) -> Vec<(String, String, String, u16, String)> {
    let mut signature = peers
        .iter()
        .map(|peer| {
            (
                peer.device_id.clone(),
                peer.device_name.clone(),
                peer.address.clone(),
                peer.port,
                peer.fingerprint.clone(),
            )
        })
        .collect::<Vec<_>>();
    signature.sort_unstable();
    signature
}

impl RuntimeResources {
    fn shutdown(&mut self) {
        self.advertiser.take();
        self.advertiser_key = None;
        if let Some(endpoint) = self.endpoint.take() {
            endpoint.close(0u32.into(), b"shutdown");
        }
        self.endpoint_port = None;
        self.identity = None;
    }
}
