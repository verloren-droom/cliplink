use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    constants::{
        app::LOCAL_HOSTNAME,
        limits::MAX_HISTORY_SNAPSHOT_ITEMS,
        transfer::{
            FILE_STREAM_CHUNK_BYTES, MAX_PROTOCOL_FRAME_BYTES, PROGRESS_EMIT_GRANULARITY_BYTES,
        },
    },
    core::{
        config::AppConfig,
        error::{AppError, AppResult},
        model::{ClipboardItem, ClipboardPayload, DiscoveredPeer},
        protocol::{TransferRequest, TransferRequestBody, TransferResponse, TransferResponseBody},
        security::certificate_fingerprint,
        storage::HistoryStore,
    },
};
use crossbeam_channel::Sender;
use parking_lot::RwLock;
use quinn::{Connection, Endpoint, RecvStream, SendStream};
use rustls::pki_types::CertificateDer;
use serde::{Serialize, de::DeserializeOwned};
use tokio::{
    fs::{self, File},
    io::{AsyncReadExt, AsyncWriteExt},
};
use uuid::Uuid;

use super::{FileTransferProgress, RuntimeEvent};

#[derive(Clone)]
pub(super) struct CachedPeerConnection {
    pub(super) address: SocketAddr,
    pub(super) fingerprint: String,
    pub(super) connection: Connection,
}

pub(super) type PeerConnectionPool = Arc<RwLock<HashMap<String, CachedPeerConnection>>>;

pub(super) async fn handle_connection(
    connection: Result<Connection, quinn::ConnectionError>,
    history_store: Arc<HistoryStore>,
    config_state: Arc<RwLock<AppConfig>>,
    inbox_dir: PathBuf,
    event_tx: Sender<RuntimeEvent>,
) -> AppResult<()> {
    let connection = connection.map_err(|error| AppError::Network(error.to_string()))?;
    let peer_fingerprint = connection_peer_fingerprint(&connection);
    loop {
        let stream = connection.accept_bi().await;
        let (mut send, mut recv) = match stream {
            Ok(stream) => stream,
            Err(quinn::ConnectionError::ApplicationClosed { .. }) => return Ok(()),
            Err(error) => return Err(AppError::Network(error.to_string())),
        };

        let request: TransferRequest = read_json_frame(&mut recv).await?;
        request.validate()?;

        match request.body {
            TransferRequestBody::PushClipboard {
                item,
                include_files,
            } => {
                let item =
                    receive_wire_clipboard_item(&mut recv, item, include_files, &inbox_dir, None)
                        .await?;
                let _ = event_tx.send(RuntimeEvent::InboundClipboard(item));
                write_json_frame(&mut send, &TransferResponse::ack()).await?;
            }
            TransferRequestBody::RemoveHistoryItems { item_ids } => {
                if let Some(fingerprint) = peer_fingerprint.as_deref() {
                    let peer_identity = config_state.read().trusted_peers.iter().find_map(|peer| {
                        (peer.fingerprint == fingerprint)
                            .then(|| (peer.device_id.clone(), peer.device_name.clone()))
                    });
                    if let Some((peer_device_id, peer_device_name)) = peer_identity {
                        let _ = event_tx.send(RuntimeEvent::RemoteHistoryItemsRemoved {
                            peer_device_id,
                            peer_device_name,
                            item_ids,
                        });
                    }
                }
                write_json_frame(&mut send, &TransferResponse::ack()).await?;
            }
            TransferRequestBody::UpdateShareState {
                share_local_history,
            } => {
                if let Some(fingerprint) = peer_fingerprint.as_deref() {
                    let peer_identity = config_state.read().trusted_peers.iter().find_map(|peer| {
                        (peer.fingerprint == fingerprint)
                            .then(|| (peer.device_id.clone(), peer.device_name.clone()))
                    });
                    if let Some((peer_device_id, peer_device_name)) = peer_identity {
                        let _ = event_tx.send(RuntimeEvent::RemoteShareStateChanged {
                            peer_device_id,
                            peer_device_name,
                            share_local_history,
                        });
                    }
                }
                write_json_frame(&mut send, &TransferResponse::ack()).await?;
            }
            TransferRequestBody::RevokeTrust => {
                if let Some(fingerprint) = peer_fingerprint.as_deref() {
                    let revoked_peer = config_state.read().trusted_peers.iter().find_map(|peer| {
                        (peer.fingerprint == fingerprint)
                            .then(|| (peer.device_id.clone(), peer.device_name.clone()))
                    });
                    if let Some((peer_device_id, peer_device_name)) = revoked_peer {
                        let _ = event_tx.send(RuntimeEvent::TrustRevokedByPeer {
                            peer_device_id,
                            peer_device_name,
                        });
                    }
                }
                write_json_frame(&mut send, &TransferResponse::ack()).await?;
            }
            TransferRequestBody::FetchHistorySnapshot { limit } => {
                let items = if config_state.read().share_local_history {
                    history_store
                        .recent(limit.clamp(1, MAX_HISTORY_SNAPSHOT_ITEMS))?
                        .into_iter()
                        .filter(|item| !item.is_remote)
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                write_json_frame(&mut send, &TransferResponse::history_snapshot(&items)).await?;
            }
            TransferRequestBody::FetchClipboardItem { item_id } => {
                if !config_state.read().share_local_history {
                    write_json_frame(
                        &mut send,
                        &TransferResponse::error("Remote clipboard sharing is disabled."),
                    )
                    .await?;
                    send.finish()
                        .map_err(|error| AppError::Network(error.to_string()))?;
                    continue;
                }

                let item = history_store
                    .find_by_id(item_id)?
                    .filter(|item| !item.is_remote);

                let Some(item) = item else {
                    write_json_frame(
                        &mut send,
                        &TransferResponse::error("Requested clipboard history item was not found."),
                    )
                    .await?;
                    send.finish()
                        .map_err(|error| AppError::Network(error.to_string()))?;
                    continue;
                };

                write_json_frame(&mut send, &TransferResponse::clipboard_item(&item, false))
                    .await?;
            }
            TransferRequestBody::FetchFiles { item_id } => {
                if !config_state.read().share_local_history {
                    write_json_frame(
                        &mut send,
                        &TransferResponse::error("Remote file sharing is disabled."),
                    )
                    .await?;
                    send.finish()
                        .map_err(|error| AppError::Network(error.to_string()))?;
                    continue;
                }

                let item = history_store
                    .find_by_id(item_id)?
                    .filter(|item| !item.is_remote);

                let Some(item) = item else {
                    write_json_frame(
                        &mut send,
                        &TransferResponse::error("Requested file history item was not found."),
                    )
                    .await?;
                    send.finish()
                        .map_err(|error| AppError::Network(error.to_string()))?;
                    continue;
                };

                if !matches!(item.payload, ClipboardPayload::Files(_)) {
                    write_json_frame(
                        &mut send,
                        &TransferResponse::error(
                            "Requested history item does not contain transferable files.",
                        ),
                    )
                    .await?;
                    send.finish()
                        .map_err(|error| AppError::Network(error.to_string()))?;
                    continue;
                }

                write_json_frame(&mut send, &TransferResponse::clipboard_item(&item, true)).await?;
                write_item_files(&mut send, &item).await?;
            }
        }

        send.finish()
            .map_err(|error| AppError::Network(error.to_string()))?;
    }
}

pub(super) async fn push_clipboard_to_peer(
    endpoint: Endpoint,
    connection_pool: PeerConnectionPool,
    peer: DiscoveredPeer,
    item: ClipboardItem,
) -> AppResult<()> {
    let (mut send, mut recv) = open_peer_stream(endpoint, &connection_pool, &peer).await?;
    write_json_frame(&mut send, &TransferRequest::push_clipboard(&item)).await?;
    write_item_files(&mut send, &item).await?;
    send.finish()
        .map_err(|error| AppError::Network(error.to_string()))?;

    let response: TransferResponse = read_json_frame(&mut recv).await?;
    response.validate()?;
    if let TransferResponseBody::Error { message } = response.body {
        return Err(AppError::Network(message));
    }
    Ok(())
}

pub(super) async fn push_share_state_to_peer(
    endpoint: Endpoint,
    connection_pool: PeerConnectionPool,
    peer: DiscoveredPeer,
    share_local_history: bool,
) -> AppResult<()> {
    let (mut send, mut recv) = open_peer_stream(endpoint, &connection_pool, &peer).await?;
    write_json_frame(
        &mut send,
        &TransferRequest::update_share_state(share_local_history),
    )
    .await?;
    send.finish()
        .map_err(|error| AppError::Network(error.to_string()))?;

    let response: TransferResponse = read_json_frame(&mut recv).await?;
    response.validate()?;
    if let TransferResponseBody::Error { message } = response.body {
        return Err(AppError::Network(message));
    }
    Ok(())
}

pub(super) async fn push_history_removals_to_peer(
    endpoint: Endpoint,
    connection_pool: PeerConnectionPool,
    peer: DiscoveredPeer,
    item_ids: Vec<Uuid>,
) -> AppResult<()> {
    if item_ids.is_empty() {
        return Ok(());
    }

    let (mut send, mut recv) = open_peer_stream(endpoint, &connection_pool, &peer).await?;
    write_json_frame(&mut send, &TransferRequest::remove_history_items(&item_ids)).await?;
    send.finish()
        .map_err(|error| AppError::Network(error.to_string()))?;

    let response: TransferResponse = read_json_frame(&mut recv).await?;
    response.validate()?;
    if let TransferResponseBody::Error { message } = response.body {
        return Err(AppError::Network(message));
    }
    Ok(())
}

pub(super) async fn notify_peer_trust_revoked(
    endpoint: Endpoint,
    connection_pool: PeerConnectionPool,
    peer: DiscoveredPeer,
) -> AppResult<()> {
    let (mut send, mut recv) = open_peer_stream(endpoint, &connection_pool, &peer).await?;
    write_json_frame(&mut send, &TransferRequest::revoke_trust()).await?;
    send.finish()
        .map_err(|error| AppError::Network(error.to_string()))?;

    let response: TransferResponse = read_json_frame(&mut recv).await?;
    response.validate()?;
    match response.body {
        TransferResponseBody::Ack => Ok(()),
        TransferResponseBody::Error { message } => Err(AppError::Network(message)),
        _ => Err(AppError::Network(
            "Remote peer returned an unexpected trust revocation response.".to_string(),
        )),
    }
}

pub(super) async fn fetch_history_snapshot_from_peer(
    endpoint: Endpoint,
    connection_pool: PeerConnectionPool,
    peer: DiscoveredPeer,
    limit: usize,
) -> AppResult<Vec<ClipboardItem>> {
    let (mut send, mut recv) = open_peer_stream(endpoint, &connection_pool, &peer).await?;
    write_json_frame(
        &mut send,
        &TransferRequest::fetch_history_snapshot(limit.clamp(1, MAX_HISTORY_SNAPSHOT_ITEMS)),
    )
    .await?;
    send.finish()
        .map_err(|error| AppError::Network(error.to_string()))?;

    let response: TransferResponse = read_json_frame(&mut recv).await?;
    response.validate()?;
    match response.body {
        TransferResponseBody::HistorySnapshot { items } => items
            .into_iter()
            .map(|item| item.into_metadata_item())
            .collect(),
        TransferResponseBody::Error { message } => Err(AppError::Network(message)),
        _ => Err(AppError::Network(
            "Remote peer returned an unexpected history snapshot response.".to_string(),
        )),
    }
}

pub(super) async fn fetch_remote_clipboard_item_from_peer(
    endpoint: Endpoint,
    connection_pool: PeerConnectionPool,
    peer: DiscoveredPeer,
    item_id: Uuid,
) -> AppResult<ClipboardItem> {
    let (mut send, mut recv) = open_peer_stream(endpoint, &connection_pool, &peer).await?;
    write_json_frame(&mut send, &TransferRequest::fetch_clipboard_item(item_id)).await?;
    send.finish()
        .map_err(|error| AppError::Network(error.to_string()))?;

    let response: TransferResponse = read_json_frame(&mut recv).await?;
    response.validate()?;
    match response.body {
        TransferResponseBody::ClipboardItem {
            item: wire_item,
            include_files,
        } => {
            receive_wire_clipboard_item(&mut recv, wire_item, include_files, Path::new("."), None)
                .await
        }
        TransferResponseBody::Error { message } => Err(AppError::Network(message)),
        _ => Err(AppError::Network(
            "Remote peer returned an unexpected clipboard item response.".to_string(),
        )),
    }
}

pub(super) async fn fetch_remote_files_from_peer(
    endpoint: Endpoint,
    connection_pool: PeerConnectionPool,
    peer: DiscoveredPeer,
    item: ClipboardItem,
    inbox_dir: &Path,
    event_tx: &Sender<RuntimeEvent>,
) -> AppResult<ClipboardItem> {
    let request_item_id = item.id;
    let (mut send, mut recv) = open_peer_stream(endpoint, &connection_pool, &peer).await?;
    write_json_frame(&mut send, &TransferRequest::fetch_files(request_item_id)).await?;
    send.finish()
        .map_err(|error| AppError::Network(error.to_string()))?;

    let response: TransferResponse = read_json_frame(&mut recv).await?;
    response.validate()?;
    match response.body {
        TransferResponseBody::ClipboardItem {
            item: wire_item,
            include_files,
        } => {
            let progress = ProgressReporter::new(
                event_tx.clone(),
                request_item_id,
                peer.device_name.clone(),
                item.summary.clone(),
                wire_item.files.iter().map(|file| file.size_bytes).sum(),
            );
            receive_wire_clipboard_item(
                &mut recv,
                wire_item,
                include_files,
                inbox_dir,
                Some(progress),
            )
            .await
        }
        TransferResponseBody::Error { message } => Err(AppError::Network(message)),
        _ => Err(AppError::Network(
            "Remote peer returned an unexpected file transfer response.".to_string(),
        )),
    }
}

pub(super) fn close_peer_connection_pool(connection_pool: &PeerConnectionPool) {
    let mut pool = connection_pool.write();
    for (_, entry) in pool.drain() {
        entry.connection.close(0u32.into(), b"shutdown");
    }
}

async fn open_peer_stream(
    endpoint: Endpoint,
    connection_pool: &PeerConnectionPool,
    peer: &DiscoveredPeer,
) -> AppResult<(SendStream, RecvStream)> {
    let address = parse_peer_addr(peer)?;
    if let Some(connection) = cached_peer_connection(connection_pool, peer, address) {
        match connection.open_bi().await {
            Ok(stream) => return Ok(stream),
            Err(_) => {
                evict_cached_peer_connection(connection_pool, peer, address);
            }
        }
    }

    let connection = connect_to_peer(endpoint, address).await?;
    let connection_key = peer_connection_key(peer);
    {
        let mut pool = connection_pool.write();
        if !peer.fingerprint.is_empty() {
            pool.retain(|key, entry| {
                !(entry.fingerprint == peer.fingerprint && key != connection_key.as_str())
            });
        }
        pool.insert(
            connection_key,
            CachedPeerConnection {
                address,
                fingerprint: peer.fingerprint.clone(),
                connection: connection.clone(),
            },
        );
    }
    connection
        .open_bi()
        .await
        .map_err(|error| AppError::Network(error.to_string()))
}

fn cached_peer_connection(
    connection_pool: &PeerConnectionPool,
    peer: &DiscoveredPeer,
    address: SocketAddr,
) -> Option<Connection> {
    let pool = connection_pool.read();
    let entry = pool.get(peer_connection_key_ref(
        peer.device_id.as_str(),
        peer.fingerprint.as_str(),
    ))?;
    if entry.address != address
        || entry.fingerprint != peer.fingerprint
        || entry.connection.close_reason().is_some()
    {
        return None;
    }
    Some(entry.connection.clone())
}

fn evict_cached_peer_connection(
    connection_pool: &PeerConnectionPool,
    peer: &DiscoveredPeer,
    address: SocketAddr,
) {
    let mut pool = connection_pool.write();
    let key = peer_connection_key_ref(peer.device_id.as_str(), peer.fingerprint.as_str());
    let should_remove = pool
        .get(key)
        .is_some_and(|entry| entry.address == address && entry.fingerprint == peer.fingerprint);
    if should_remove {
        pool.remove(key);
    }
}

fn peer_connection_key(peer: &DiscoveredPeer) -> String {
    peer_connection_key_ref(peer.device_id.as_str(), peer.fingerprint.as_str()).to_string()
}

fn peer_connection_key_ref<'a>(device_id: &'a str, fingerprint: &'a str) -> &'a str {
    if fingerprint.is_empty() {
        device_id
    } else {
        fingerprint
    }
}

async fn connect_to_peer(endpoint: Endpoint, address: SocketAddr) -> AppResult<Connection> {
    endpoint
        .connect(address, LOCAL_HOSTNAME)
        .map_err(|error| AppError::Network(error.to_string()))?
        .await
        .map_err(|error| AppError::Network(error.to_string()))
}

async fn receive_wire_clipboard_item(
    recv: &mut RecvStream,
    wire_item: crate::core::protocol::WireClipboardItem,
    include_files: bool,
    inbox_dir: &Path,
    progress: Option<ProgressReporter>,
) -> AppResult<ClipboardItem> {
    if matches!(wire_item.kind, crate::core::model::ClipboardKind::Files) && include_files {
        let base_dir = inbox_dir.join(wire_item.id.to_string());
        match fs::remove_dir_all(&base_dir).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        fs::create_dir_all(&base_dir).await?;

        let mut local_files = Vec::with_capacity(wire_item.files.len());
        let mut progress = progress;
        if let Some(progress) = progress.as_mut() {
            progress.emit(0);
        }

        for file in &wire_item.files {
            let safe_relative = crate::core::clipboard::sanitize_relative_path(&file.relative_path);
            let output_path = base_dir.join(safe_relative);
            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent).await?;
            }

            let mut output = File::create(&output_path).await?;
            let mut remaining = file.size_bytes;
            let mut buffer = vec![0_u8; FILE_STREAM_CHUNK_BYTES];
            while remaining > 0 {
                let to_read = remaining.min(buffer.len() as u64) as usize;
                recv.read_exact(&mut buffer[..to_read])
                    .await
                    .map_err(|error| AppError::Network(error.to_string()))?;
                output.write_all(&buffer[..to_read]).await?;
                remaining -= to_read as u64;
                if let Some(progress) = progress.as_mut() {
                    progress.advance(to_read as u64);
                }
            }
            local_files.push(output_path);
        }

        if let Some(progress) = progress.as_mut() {
            progress.finish();
        }
        wire_item.into_item_with_local_files(local_files)
    } else {
        wire_item.into_metadata_item()
    }
}

async fn write_item_files(send: &mut SendStream, item: &ClipboardItem) -> AppResult<()> {
    if let ClipboardPayload::Files(files) = &item.payload {
        let mut buffer = vec![0_u8; FILE_STREAM_CHUNK_BYTES];
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

async fn write_json_frame<T: Serialize>(send: &mut SendStream, value: &T) -> AppResult<()> {
    let frame = serde_json::to_vec(value)?;
    if frame.is_empty() || frame.len() > MAX_PROTOCOL_FRAME_BYTES {
        return Err(AppError::Network(format!(
            "Protocol frame size {} is invalid.",
            frame.len()
        )));
    }
    send.write_all(&(frame.len() as u32).to_be_bytes())
        .await
        .map_err(|error| AppError::Network(error.to_string()))?;
    send.write_all(&frame)
        .await
        .map_err(|error| AppError::Network(error.to_string()))?;
    Ok(())
}

async fn read_json_frame<T: DeserializeOwned>(recv: &mut RecvStream) -> AppResult<T> {
    let mut len_buf = [0_u8; 4];
    recv.read_exact(&mut len_buf)
        .await
        .map_err(|error| AppError::Network(error.to_string()))?;
    let frame_len = u32::from_be_bytes(len_buf) as usize;
    if frame_len == 0 || frame_len > MAX_PROTOCOL_FRAME_BYTES {
        return Err(AppError::Network(format!(
            "Protocol frame length {frame_len} is invalid."
        )));
    }
    let mut frame = vec![0_u8; frame_len];
    recv.read_exact(&mut frame)
        .await
        .map_err(|error| AppError::Network(error.to_string()))?;
    serde_json::from_slice(&frame).map_err(Into::into)
}

fn parse_peer_addr(peer: &DiscoveredPeer) -> AppResult<SocketAddr> {
    let ip = peer
        .address
        .parse::<IpAddr>()
        .map_err(|error| AppError::Network(error.to_string()))?;
    Ok(SocketAddr::new(ip, peer.port))
}

fn connection_peer_fingerprint(connection: &Connection) -> Option<String> {
    let identity = connection.peer_identity()?;
    let certs = identity.downcast::<Vec<CertificateDer<'static>>>().ok()?;
    certs
        .first()
        .map(|certificate| certificate_fingerprint(certificate.as_ref()))
}

struct ProgressReporter {
    event_tx: Sender<RuntimeEvent>,
    snapshot: FileTransferProgress,
    next_emit_at: u64,
}

impl ProgressReporter {
    fn new(
        event_tx: Sender<RuntimeEvent>,
        item_id: Uuid,
        source_device_name: String,
        summary: String,
        bytes_total: u64,
    ) -> Self {
        Self {
            event_tx,
            snapshot: FileTransferProgress {
                item_id,
                source_device_name,
                summary,
                bytes_done: 0,
                bytes_total,
            },
            next_emit_at: 0,
        }
    }

    fn emit(&mut self, bytes_done: u64) {
        self.snapshot.bytes_done = bytes_done.min(self.snapshot.bytes_total);
        let should_emit = self.snapshot.bytes_done >= self.next_emit_at
            || self.snapshot.bytes_done == self.snapshot.bytes_total;
        if should_emit {
            let _ = self
                .event_tx
                .send(RuntimeEvent::FileTransferProgress(self.snapshot.clone()));
            self.next_emit_at = self
                .snapshot
                .bytes_done
                .saturating_add(PROGRESS_EMIT_GRANULARITY_BYTES);
        }
    }

    fn advance(&mut self, bytes_delta: u64) {
        self.emit(self.snapshot.bytes_done.saturating_add(bytes_delta));
    }

    fn finish(&mut self) {
        self.emit(self.snapshot.bytes_total);
    }
}
