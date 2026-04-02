use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::{
    constants::timing::QUIC_KEEP_ALIVE_INTERVAL,
    constants::transfer::{
        QUIC_CONNECTION_RECEIVE_WINDOW_BYTES, QUIC_MAX_CONCURRENT_BIDI_STREAMS,
        QUIC_SEND_WINDOW_BYTES, QUIC_STREAM_RECEIVE_WINDOW_BYTES,
    },
    core::{
        at_rest::LocalDataCipher,
        config::AppConfig,
        error::{AppError, AppResult},
        model::DiscoveredPeer,
        paths::AppPaths,
        security::{
            DeviceIdentity, build_rustls_client_config, build_rustls_server_config,
            load_or_create_identity,
        },
        storage::HistoryStore,
    },
};
use crossbeam_channel::Sender;
use parking_lot::RwLock;
use quinn::{
    ClientConfig, Endpoint, ServerConfig, TransportConfig,
    crypto::rustls::{QuicClientConfig, QuicServerConfig},
};

use super::{
    RuntimeEvent, RuntimeShared,
    transfer::{PeerConnectionPool, close_peer_connection_pool, handle_connection},
};

#[derive(Default)]
pub(super) struct RuntimeResources {
    pub(super) identity: Option<DeviceIdentity>,
    pub(super) endpoint: Option<Endpoint>,
    pub(super) endpoint_port: Option<u16>,
    pub(super) history_store: Option<Arc<HistoryStore>>,
    pub(super) peer_connections: PeerConnectionPool,
}

pub(super) fn trusted_fingerprint_set(config: &AppConfig) -> HashSet<String> {
    config
        .trusted_peers
        .iter()
        .map(|peer| peer.fingerprint.clone())
        .collect()
}

pub(super) fn is_trusted_peer(config: &AppConfig, peer: &DiscoveredPeer) -> bool {
    config
        .trusted_peers
        .iter()
        .any(|trusted| trusted.fingerprint == peer.fingerprint)
}

pub(super) fn resolve_trusted_peer(
    discovered: &Arc<RwLock<HashMap<String, DiscoveredPeer>>>,
    trusted_fingerprints: &Arc<RwLock<HashSet<String>>>,
    device_id: &str,
) -> AppResult<DiscoveredPeer> {
    let peer =
        discovered.read().get(device_id).cloned().ok_or_else(|| {
            AppError::Network(format!("Trusted peer `{device_id}` is not online."))
        })?;
    if !trusted_fingerprints.read().contains(&peer.fingerprint) {
        return Err(AppError::Network(format!(
            "Peer `{}` is not trusted.",
            peer.device_name
        )));
    }
    Ok(peer)
}

pub(super) async fn reconcile_runtime_resources(
    paths: &AppPaths,
    config: &AppConfig,
    local_data_cipher: &LocalDataCipher,
    history_store_profile: crate::core::storage::HistoryStoreProfile,
    config_state: Arc<RwLock<AppConfig>>,
    shared: &RuntimeShared,
    resources: &mut RuntimeResources,
) -> AppResult<()> {
    let transfer_requested = transfer_service_enabled(config);

    if !config.discovery_enabled && !transfer_requested {
        resources.shutdown();
        return Ok(());
    }

    if transfer_requested {
        let history_store =
            resources.history_store(paths, local_data_cipher, history_store_profile)?;
        if resources.identity.is_none() {
            resources.identity = Some(load_or_create_identity(
                paths,
                &config.device_id,
                &shared.local_data_cipher,
            )?);
        }
        let identity = resources.identity.as_ref().ok_or_else(|| {
            AppError::Crypto("Runtime identity was not initialized after creation.".to_string())
        })?;
        let needs_restart =
            resources.endpoint.is_none() || resources.endpoint_port != Some(config.listen_port);
        if needs_restart {
            close_peer_connection_pool(&resources.peer_connections);
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
                history_store,
                config_state,
                shared.inbox_dir.clone(),
                shared.event_tx.clone(),
                shared.shutdown.clone(),
            );
            resources.endpoint = Some(endpoint);
            resources.endpoint_port = Some(config.listen_port);
        }
    } else if let Some(endpoint) = resources.endpoint.take() {
        close_peer_connection_pool(&resources.peer_connections);
        endpoint.close(0u32.into(), b"disabled");
        resources.endpoint_port = None;
        resources.history_store = None;
        resources.identity = None;
    } else {
        close_peer_connection_pool(&resources.peer_connections);
        resources.history_store = None;
        resources.identity = None;
    }

    Ok(())
}

fn transfer_service_enabled(config: &AppConfig) -> bool {
    !config.trusted_peers.is_empty()
}

fn spawn_accept_loop(
    endpoint: Endpoint,
    history_store: Arc<HistoryStore>,
    config_state: Arc<RwLock<AppConfig>>,
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
            let history_store = history_store.clone();
            let config_state = config_state.clone();
            let inbox_dir = inbox_dir.read().clone();
            tokio::spawn(async move {
                if let Err(error) = handle_connection(
                    incoming.await,
                    history_store,
                    config_state,
                    inbox_dir,
                    event_tx.clone(),
                )
                .await
                {
                    let _ = event_tx.send(RuntimeEvent::Status(format!("接收连接失败: {error}")));
                }
            });
        }
    });
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
    let mut transport = TransportConfig::default();
    transport
        .max_concurrent_bidi_streams(QUIC_MAX_CONCURRENT_BIDI_STREAMS.into())
        .max_concurrent_uni_streams(0_u8.into())
        .stream_receive_window(QUIC_STREAM_RECEIVE_WINDOW_BYTES.into())
        .receive_window(QUIC_CONNECTION_RECEIVE_WINDOW_BYTES.into())
        .send_window(QUIC_SEND_WINDOW_BYTES)
        .keep_alive_interval(Some(QUIC_KEEP_ALIVE_INTERVAL));
    let transport = Arc::new(transport);
    server_config.transport_config(transport.clone());

    let mut endpoint = Endpoint::server(
        server_config,
        std::net::SocketAddr::new(std::net::IpAddr::from([0, 0, 0, 0]), port),
    )
    .map_err(|error| AppError::Network(error.to_string()))?;
    let mut client_config = ClientConfig::new(Arc::new(
        QuicClientConfig::try_from(client_crypto)
            .map_err(|error| AppError::Crypto(error.to_string()))?,
    ));
    client_config.transport_config(transport);
    endpoint.set_default_client_config(client_config);
    Ok(endpoint)
}

impl RuntimeResources {
    fn history_store(
        &mut self,
        paths: &AppPaths,
        local_data_cipher: &LocalDataCipher,
        profile: crate::core::storage::HistoryStoreProfile,
    ) -> AppResult<Arc<HistoryStore>> {
        if let Some(history_store) = self.history_store.as_ref() {
            return Ok(history_store.clone());
        }

        let history_store = Arc::new(HistoryStore::open(
            paths,
            local_data_cipher.clone(),
            profile,
        )?);
        self.history_store = Some(history_store.clone());
        Ok(history_store)
    }

    pub(super) fn shutdown(&mut self) {
        close_peer_connection_pool(&self.peer_connections);
        if let Some(endpoint) = self.endpoint.take() {
            endpoint.close(0u32.into(), b"shutdown");
        }
        self.endpoint_port = None;
        self.identity = None;
        self.history_store = None;
    }
}
