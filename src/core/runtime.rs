mod discovery;
mod endpoint;
mod transfer;

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use self::{
    discovery::{DiscoveryCommand, DiscoveryLoopContext, spawn_discovery_loop},
    endpoint::{
        RuntimeResources, reconcile_runtime_resources, resolve_trusted_peer,
        trusted_fingerprint_set,
    },
    transfer::{
        fetch_history_snapshot_from_peer, fetch_remote_clipboard_item_from_peer,
        fetch_remote_files_from_peer, notify_peer_trust_revoked, push_clipboard_to_peer,
        push_history_removals_to_peer, push_share_state_to_peer,
    },
};
use crate::{
    constants::limits::MAX_HISTORY_SNAPSHOT_ITEMS,
    core::{
        at_rest::LocalDataCipher,
        config::AppConfig,
        error::{AppError, AppResult},
        model::{ClipboardItem, DiscoveredPeer},
        paths::AppPaths,
        storage::HistoryStoreProfile,
    },
};
use crossbeam_channel::{Receiver, Sender};
use parking_lot::RwLock;
use tokio::{
    runtime::Builder,
    sync::{
        Notify,
        mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
    },
};
use uuid::Uuid;

/// Controller-to-runtime commands for live sync, snapshots, and remote file resolution.
#[derive(Debug, Clone)]
pub enum RuntimeCommand {
    Broadcast(ClipboardItem),
    BroadcastHistoryRemovals {
        item_ids: Vec<Uuid>,
    },
    ReplaceConfig(AppConfig),
    NotifyShareState {
        share_local_history: bool,
    },
    RequestTrust {
        peer_device_id: String,
    },
    NotifyTrustRevoked {
        peer_device_id: String,
    },
    ResolveTrustRequest {
        request_id: Uuid,
        allow: bool,
    },
    RequestHistorySnapshot {
        peer_device_id: String,
        limit: usize,
    },
    FetchRemoteClipboardItem {
        peer_device_id: String,
        item_id: Uuid,
    },
    FetchRemoteFiles(ClipboardItem),
    Shutdown,
}

/// Runtime-to-controller events emitted by background discovery and transfer tasks.
#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    PeerList(Vec<DiscoveredPeer>),
    TrustRequestReceived {
        request_id: Uuid,
        peer: DiscoveredPeer,
    },
    TrustRequestCompleted {
        peer: DiscoveredPeer,
        accepted: bool,
    },
    TrustRequestFailed {
        peer_device_id: String,
        message: String,
    },
    TrustRevokedByPeer {
        peer_device_id: String,
        peer_device_name: String,
    },
    RemoteShareStateChanged {
        peer_device_id: String,
        peer_device_name: String,
        share_local_history: bool,
    },
    InboundClipboard(ClipboardItem),
    RemoteHistoryItemsRemoved {
        peer_device_id: String,
        peer_device_name: String,
        item_ids: Vec<Uuid>,
    },
    HistorySnapshotReceived {
        peer_device_id: String,
        peer_device_name: String,
        items: Vec<ClipboardItem>,
    },
    HistorySnapshotFailed {
        peer_device_id: String,
        message: String,
    },
    RemoteClipboardResolved(ClipboardItem),
    RemoteClipboardFailed {
        item_id: Uuid,
        message: String,
    },
    RemoteFilesResolved(ClipboardItem),
    RemoteFilesFailed {
        item_id: Uuid,
        message: String,
    },
    FileTransferProgress(FileTransferProgress),
    Status(String),
}

/// Progress snapshot for a currently running remote file download.
#[derive(Debug, Clone)]
pub struct FileTransferProgress {
    pub item_id: Uuid,
    pub source_device_name: String,
    pub summary: String,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

#[derive(Debug)]
pub struct RuntimeBridge {
    command_tx: UnboundedSender<RuntimeCommand>,
    event_rx: Receiver<RuntimeEvent>,
}

struct RuntimeShared {
    local_data_cipher: LocalDataCipher,
    trusted_fingerprints: Arc<RwLock<HashSet<String>>>,
    inbox_dir: Arc<RwLock<PathBuf>>,
    event_tx: Sender<RuntimeEvent>,
    discovery_wakeup: Arc<Notify>,
    discovery_command_tx: UnboundedSender<DiscoveryCommand>,
    shutdown: Arc<AtomicBool>,
}

impl RuntimeBridge {
    pub fn start(
        paths: AppPaths,
        config: AppConfig,
        local_data_cipher: LocalDataCipher,
        history_store_profile: HistoryStoreProfile,
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
                            history_store_profile,
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
    history_store_profile: HistoryStoreProfile,
    mut command_rx: UnboundedReceiver<RuntimeCommand>,
    event_tx: Sender<RuntimeEvent>,
) -> AppResult<()> {
    let shutdown = Arc::new(AtomicBool::new(false));
    let config_state = Arc::new(RwLock::new(config));
    let discovered = Arc::new(RwLock::new(HashMap::<String, DiscoveredPeer>::new()));
    let (discovery_command_tx, discovery_command_rx) = unbounded_channel();
    let mut resources = RuntimeResources::default();
    let shared = RuntimeShared {
        local_data_cipher,
        trusted_fingerprints: Arc::new(RwLock::new(trusted_fingerprint_set(&config_state.read()))),
        inbox_dir: Arc::new(RwLock::new(paths.inbox_dir.clone())),
        event_tx: event_tx.clone(),
        discovery_wakeup: Arc::new(Notify::new()),
        discovery_command_tx,
        shutdown: shutdown.clone(),
    };
    std::fs::create_dir_all(shared.inbox_dir.read().as_path())?;

    let initial_config = config_state.read().clone();
    reconcile_runtime_resources(
        &paths,
        &initial_config,
        &shared.local_data_cipher,
        history_store_profile,
        config_state.clone(),
        &shared,
        &mut resources,
    )
    .await?;

    spawn_discovery_loop(
        DiscoveryLoopContext {
            paths: paths.clone(),
            local_data_cipher: shared.local_data_cipher.clone(),
            config_state: config_state.clone(),
            discovered: discovered.clone(),
            event_tx: event_tx.clone(),
            wakeup: shared.discovery_wakeup.clone(),
            shutdown: shutdown.clone(),
        },
        discovery_command_rx,
    );

    while let Some(command) = command_rx.recv().await {
        match command {
            RuntimeCommand::Broadcast(item) => {
                if !config_state.read().share_local_history {
                    continue;
                }
                let Some(endpoint) = resources.endpoint.as_ref().cloned() else {
                    continue;
                };

                let peers = trusted_online_peers(&discovered, &shared.trusted_fingerprints);

                for peer in peers {
                    let endpoint = endpoint.clone();
                    let connection_pool = resources.peer_connections.clone();
                    let event_tx = shared.event_tx.clone();
                    let item = item.clone();
                    tokio::spawn(async move {
                        if let Err(error) =
                            push_clipboard_to_peer(endpoint, connection_pool, peer.clone(), item)
                                .await
                        {
                            let _ = event_tx.send(RuntimeEvent::Status(format!(
                                "同步到 {} 失败: {}",
                                peer.device_name, error
                            )));
                        }
                    });
                }
            }
            RuntimeCommand::BroadcastHistoryRemovals { item_ids } => {
                if item_ids.is_empty() || !config_state.read().share_local_history {
                    continue;
                }
                let Some(endpoint) = resources.endpoint.as_ref().cloned() else {
                    continue;
                };

                let peers = trusted_online_peers(&discovered, &shared.trusted_fingerprints);

                for peer in peers {
                    let endpoint = endpoint.clone();
                    let connection_pool = resources.peer_connections.clone();
                    let event_tx = shared.event_tx.clone();
                    let item_ids = item_ids.clone();
                    tokio::spawn(async move {
                        if let Err(error) = push_history_removals_to_peer(
                            endpoint,
                            connection_pool,
                            peer.clone(),
                            item_ids,
                        )
                        .await
                        {
                            let _ = event_tx.send(RuntimeEvent::Status(format!(
                                "同步 {} 的历史删除事件失败: {}",
                                peer.device_name, error
                            )));
                        }
                    });
                }
            }
            RuntimeCommand::NotifyShareState {
                share_local_history,
            } => {
                let Some(endpoint) = resources.endpoint.as_ref().cloned() else {
                    continue;
                };

                let peers = trusted_online_peers(&discovered, &shared.trusted_fingerprints);

                for peer in peers {
                    let endpoint = endpoint.clone();
                    let connection_pool = resources.peer_connections.clone();
                    let event_tx = shared.event_tx.clone();
                    tokio::spawn(async move {
                        if let Err(error) = push_share_state_to_peer(
                            endpoint,
                            connection_pool,
                            peer.clone(),
                            share_local_history,
                        )
                        .await
                        {
                            let _ = event_tx.send(RuntimeEvent::Status(format!(
                                "通知 {} 更新共享状态失败: {}",
                                peer.device_name, error
                            )));
                        }
                    });
                }
            }
            RuntimeCommand::RequestTrust { peer_device_id } => {
                let Some(peer) = discovered.read().get(&peer_device_id).cloned() else {
                    let _ = shared.event_tx.send(RuntimeEvent::TrustRequestFailed {
                        peer_device_id,
                        message: "The selected device is not online.".to_string(),
                    });
                    continue;
                };

                if let Err(error) = shared
                    .discovery_command_tx
                    .send(DiscoveryCommand::SendTrustRequest { peer })
                {
                    let _ = shared.event_tx.send(RuntimeEvent::TrustRequestFailed {
                        peer_device_id,
                        message: error.to_string(),
                    });
                }
            }
            RuntimeCommand::NotifyTrustRevoked { peer_device_id } => {
                let Some(endpoint) = resources.endpoint.as_ref().cloned() else {
                    continue;
                };
                let peer = match resolve_trusted_peer(
                    &discovered,
                    &shared.trusted_fingerprints,
                    &peer_device_id,
                ) {
                    Ok(peer) => peer,
                    Err(_) => continue,
                };
                if let Err(error) = notify_peer_trust_revoked(
                    endpoint,
                    resources.peer_connections.clone(),
                    peer.clone(),
                )
                .await
                {
                    let _ = shared.event_tx.send(RuntimeEvent::Status(format!(
                        "通知设备 {} 移除信任失败: {error}",
                        peer.device_name
                    )));
                }
            }
            RuntimeCommand::ResolveTrustRequest { request_id, allow } => {
                if let Err(error) = shared
                    .discovery_command_tx
                    .send(DiscoveryCommand::SendTrustDecision { request_id, allow })
                {
                    let _ = shared
                        .event_tx
                        .send(RuntimeEvent::Status(format!("发送信任确认失败: {error}")));
                }
            }
            RuntimeCommand::ReplaceConfig(new_config) => {
                let old_port = config_state.read().listen_port;
                *config_state.write() = new_config.clone();
                *shared.trusted_fingerprints.write() = trusted_fingerprint_set(&new_config);
                *shared.inbox_dir.write() = paths.inbox_dir.clone();
                if let Err(error) = reconcile_runtime_resources(
                    &paths,
                    &new_config,
                    &shared.local_data_cipher,
                    history_store_profile,
                    config_state.clone(),
                    &shared,
                    &mut resources,
                )
                .await
                {
                    let _ = shared.event_tx.send(RuntimeEvent::Status(format!(
                        "后台网络服务更新失败: {error}"
                    )));
                } else if old_port != new_config.listen_port {
                    let _ = shared
                        .event_tx
                        .send(RuntimeEvent::Status("监听端口已更新".to_string()));
                }
                shared.discovery_wakeup.notify_one();
            }
            RuntimeCommand::RequestHistorySnapshot {
                peer_device_id,
                limit,
            } => {
                let Some(endpoint) = resources.endpoint.as_ref().cloned() else {
                    let _ = shared.event_tx.send(RuntimeEvent::HistorySnapshotFailed {
                        peer_device_id,
                        message: "传输服务未启动".to_string(),
                    });
                    continue;
                };

                let peer = match resolve_trusted_peer(
                    &discovered,
                    &shared.trusted_fingerprints,
                    &peer_device_id,
                ) {
                    Ok(peer) => peer,
                    Err(error) => {
                        let _ = shared.event_tx.send(RuntimeEvent::HistorySnapshotFailed {
                            peer_device_id,
                            message: error.to_string(),
                        });
                        continue;
                    }
                };
                let event_tx = shared.event_tx.clone();
                let connection_pool = resources.peer_connections.clone();
                let requested_device_id = peer.device_id.clone();
                let requested_device_name = peer.device_name.clone();
                tokio::spawn(async move {
                    match fetch_history_snapshot_from_peer(
                        endpoint,
                        connection_pool,
                        peer,
                        limit.clamp(1, MAX_HISTORY_SNAPSHOT_ITEMS),
                    )
                    .await
                    {
                        Ok(items) => {
                            let _ = event_tx.send(RuntimeEvent::HistorySnapshotReceived {
                                peer_device_id: requested_device_id,
                                peer_device_name: requested_device_name,
                                items,
                            });
                        }
                        Err(error) => {
                            let _ = event_tx.send(RuntimeEvent::HistorySnapshotFailed {
                                peer_device_id: requested_device_id,
                                message: error.to_string(),
                            });
                        }
                    }
                });
            }
            RuntimeCommand::FetchRemoteClipboardItem {
                peer_device_id,
                item_id,
            } => {
                let Some(endpoint) = resources.endpoint.as_ref().cloned() else {
                    let _ = shared.event_tx.send(RuntimeEvent::RemoteClipboardFailed {
                        item_id,
                        message: "传输服务未启动".to_string(),
                    });
                    continue;
                };

                let peer = match resolve_trusted_peer(
                    &discovered,
                    &shared.trusted_fingerprints,
                    &peer_device_id,
                ) {
                    Ok(peer) => peer,
                    Err(error) => {
                        let _ = shared.event_tx.send(RuntimeEvent::RemoteClipboardFailed {
                            item_id,
                            message: error.to_string(),
                        });
                        continue;
                    }
                };

                let event_tx = shared.event_tx.clone();
                let connection_pool = resources.peer_connections.clone();
                tokio::spawn(async move {
                    match fetch_remote_clipboard_item_from_peer(
                        endpoint,
                        connection_pool,
                        peer,
                        item_id,
                    )
                    .await
                    {
                        Ok(item) => {
                            let _ = event_tx.send(RuntimeEvent::RemoteClipboardResolved(item));
                        }
                        Err(error) => {
                            let _ = event_tx.send(RuntimeEvent::RemoteClipboardFailed {
                                item_id,
                                message: error.to_string(),
                            });
                        }
                    }
                });
            }
            RuntimeCommand::FetchRemoteFiles(item) => {
                let Some(endpoint) = resources.endpoint.as_ref().cloned() else {
                    let _ = shared.event_tx.send(RuntimeEvent::RemoteFilesFailed {
                        item_id: item.id,
                        message: "传输服务未启动".to_string(),
                    });
                    continue;
                };

                let Some(peer_device_id) = item.source_device_id.clone() else {
                    let _ = shared.event_tx.send(RuntimeEvent::RemoteFilesFailed {
                        item_id: item.id,
                        message: "远程文件缺少来源设备标识".to_string(),
                    });
                    continue;
                };
                let peer = match resolve_trusted_peer(
                    &discovered,
                    &shared.trusted_fingerprints,
                    &peer_device_id,
                ) {
                    Ok(peer) => peer,
                    Err(error) => {
                        let _ = shared.event_tx.send(RuntimeEvent::RemoteFilesFailed {
                            item_id: item.id,
                            message: error.to_string(),
                        });
                        continue;
                    }
                };

                let event_tx = shared.event_tx.clone();
                let connection_pool = resources.peer_connections.clone();
                let inbox_dir = shared.inbox_dir.read().clone();
                let request_item_id = item.id;
                tokio::spawn(async move {
                    match fetch_remote_files_from_peer(
                        endpoint,
                        connection_pool,
                        peer.clone(),
                        item,
                        &inbox_dir,
                        &event_tx,
                    )
                    .await
                    {
                        Ok(item) => {
                            let _ = event_tx.send(RuntimeEvent::RemoteFilesResolved(item));
                        }
                        Err(error) => {
                            let _ = event_tx.send(RuntimeEvent::RemoteFilesFailed {
                                item_id: request_item_id,
                                message: error.to_string(),
                            });
                        }
                    }
                });
            }
            RuntimeCommand::Shutdown => {
                shutdown.store(true, Ordering::Relaxed);
                shared.discovery_wakeup.notify_waiters();
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

fn trusted_online_peers(
    discovered: &Arc<RwLock<HashMap<String, DiscoveredPeer>>>,
    trusted_fingerprints: &Arc<RwLock<HashSet<String>>>,
) -> Vec<DiscoveredPeer> {
    let trusted = trusted_fingerprints.read().clone();
    discovered
        .read()
        .values()
        .filter(|peer| trusted.contains(&peer.fingerprint))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::discovery::{prune_stale_discovered_peers, sorted_discovered_peers};
    use crate::core::{discovery::merge_announced_peers, model::DiscoveredPeer};
    use time::{Duration, OffsetDateTime};

    fn sample_peer(device_id: &str, device_name: &str) -> DiscoveredPeer {
        DiscoveredPeer {
            device_id: device_id.to_string(),
            device_name: device_name.to_string(),
            host: format!("{device_name}.local"),
            address: "192.168.1.10".to_string(),
            port: 27_841,
            fingerprint: format!("fp-{device_id}"),
            last_seen: OffsetDateTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn discovery_keeps_peer_online_until_grace_expires() {
        let now = OffsetDateTime::now_utc();
        let mut discovered = HashMap::new();
        merge_announced_peers(
            &mut discovered,
            vec![sample_peer("android", "Android")],
            now,
        );

        prune_stale_discovered_peers(&mut discovered, now + Duration::seconds(6));
        assert_eq!(sorted_discovered_peers(&discovered).len(), 1);

        prune_stale_discovered_peers(&mut discovered, now + Duration::seconds(20));
        assert!(sorted_discovered_peers(&discovered).is_empty());
    }
}
