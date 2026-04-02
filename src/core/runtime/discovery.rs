use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::{
    constants::{
        discovery::{DISCOVERY_PORT, MAX_DISCOVERY_PACKET_BYTES},
        timing::{DISCOVERY_ANNOUNCE_INTERVAL, DISCOVERY_OFFLINE_GRACE, DISCOVERY_RETRY_INTERVAL},
    },
    core::{
        at_rest::LocalDataCipher,
        config::AppConfig,
        discovery::{
            ReceivedDiscoveryMessage, build_discovery_config, merge_announced_peers,
            open_discovery_socket,
        },
        model::DiscoveredPeer,
        paths::AppPaths,
        security::{DeviceIdentity, load_or_create_identity},
    },
};
use crossbeam_channel::Sender;
use parking_lot::RwLock;
use time::OffsetDateTime;
use tokio::{
    sync::{Notify, mpsc::UnboundedReceiver},
    time::{Instant, sleep, sleep_until},
};
use uuid::Uuid;

use super::{RuntimeEvent, endpoint::is_trusted_peer};

pub(super) struct DiscoveryLoopContext {
    pub(super) paths: AppPaths,
    pub(super) local_data_cipher: LocalDataCipher,
    pub(super) config_state: Arc<RwLock<AppConfig>>,
    pub(super) discovered: Arc<RwLock<HashMap<String, DiscoveredPeer>>>,
    pub(super) event_tx: Sender<RuntimeEvent>,
    pub(super) wakeup: Arc<Notify>,
    pub(super) shutdown: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
pub(super) enum DiscoveryCommand {
    SendTrustRequest { peer: DiscoveredPeer },
    SendTrustDecision { request_id: Uuid, allow: bool },
}

#[derive(Debug, Clone)]
struct PendingIncomingTrustRequest {
    source: SocketAddr,
    peer: DiscoveredPeer,
}

pub(super) fn spawn_discovery_loop(
    context: DiscoveryLoopContext,
    mut command_rx: UnboundedReceiver<DiscoveryCommand>,
) {
    let DiscoveryLoopContext {
        paths,
        local_data_cipher,
        config_state,
        discovered,
        event_tx,
        wakeup,
        shutdown,
    } = context;

    tokio::spawn(async move {
        enum DiscoveryLoopAction {
            Continue,
            ResetSocket,
            RequestProbe,
        }

        let mut last_signature = peer_list_signature(&[]);
        let mut last_discovery_error = None::<String>;
        let mut discovery_socket = None;
        let mut active_config = None;
        let mut cached_identity = None::<DeviceIdentity>;
        let mut cached_identity_device_id = None::<String>;
        let mut discovery_buffer = vec![0_u8; MAX_DISCOVERY_PACKET_BYTES];
        let mut announce_due_at = Instant::now();
        let mut probe_requested = true;
        let mut pending_incoming_trust_requests =
            HashMap::<Uuid, PendingIncomingTrustRequest>::new();
        let mut pending_outgoing_trust_requests = HashMap::<Uuid, DiscoveredPeer>::new();

        while !shutdown.load(Ordering::Relaxed) {
            let config = config_state.read().clone();
            if !config.discovery_enabled {
                discovery_socket.take();
                active_config.take();
                last_discovery_error = None;
                pending_incoming_trust_requests.clear();
                pending_outgoing_trust_requests.clear();
                if !last_signature.is_empty() {
                    discovered.write().clear();
                    last_signature.clear();
                    let _ = event_tx.send(RuntimeEvent::PeerList(Vec::new()));
                }

                tokio::select! {
                    _ = sleep(DISCOVERY_RETRY_INTERVAL) => {}
                    _ = wakeup.notified() => {
                        probe_requested = true;
                    }
                    command = command_rx.recv() => {
                        if command.is_none() {
                            break;
                        }
                    }
                }
                continue;
            }

            if cached_identity_device_id.as_deref() != Some(config.device_id.as_str()) {
                cached_identity = None;
                cached_identity_device_id = Some(config.device_id.clone());
            }

            let identity = match cached_identity.as_ref() {
                Some(identity) => identity,
                None => {
                    match load_or_create_identity(&paths, &config.device_id, &local_data_cipher) {
                        Ok(identity) => cached_identity.insert(identity),
                        Err(error) => {
                            report_discovery_error(
                                &event_tx,
                                &mut last_discovery_error,
                                format!("设备发现初始化失败: {error}"),
                            );
                            tokio::select! {
                                _ = sleep(DISCOVERY_RETRY_INTERVAL) => {}
                                _ = wakeup.notified() => {
                                    probe_requested = true;
                                }
                            }
                            continue;
                        }
                    }
                }
            };
            let discovery_config = build_discovery_config(&config, identity);

            if active_config.as_ref() != Some(&discovery_config) || discovery_socket.is_none() {
                match open_discovery_socket().await {
                    Ok(socket) => {
                        discovery_socket = Some(socket);
                        active_config = Some(discovery_config.clone());
                        announce_due_at = Instant::now();
                        probe_requested = true;
                        last_discovery_error = None;
                    }
                    Err(error) => {
                        report_discovery_error(
                            &event_tx,
                            &mut last_discovery_error,
                            format!("设备发现套接字启动失败: {error}"),
                        );
                        tokio::select! {
                            _ = sleep(DISCOVERY_RETRY_INTERVAL) => {}
                            _ = wakeup.notified() => {
                                probe_requested = true;
                            }
                        }
                        continue;
                    }
                }
            }

            if let Some(socket) = discovery_socket.as_ref() {
                if probe_requested {
                    if let Err(error) = socket.send_probe(&discovery_config).await {
                        report_discovery_error(
                            &event_tx,
                            &mut last_discovery_error,
                            format!("发送设备发现探测失败: {error}"),
                        );
                    }
                    if let Err(error) = socket.send_announce(&discovery_config).await {
                        report_discovery_error(
                            &event_tx,
                            &mut last_discovery_error,
                            format!("发送设备发现广播失败: {error}"),
                        );
                    }
                    announce_due_at = Instant::now() + DISCOVERY_ANNOUNCE_INTERVAL;
                    probe_requested = false;
                    refresh_discovered_peer_snapshot(
                        &discovered,
                        &event_tx,
                        &mut last_signature,
                        OffsetDateTime::now_utc(),
                    );
                    continue;
                }

                let action = tokio::select! {
                    _ = sleep_until(announce_due_at) => {
                        if let Err(error) = socket.send_announce(&discovery_config).await {
                            report_discovery_error(
                                &event_tx,
                                &mut last_discovery_error,
                                format!("发送设备发现广播失败: {error}"),
                            );
                            DiscoveryLoopAction::ResetSocket
                        } else {
                            last_discovery_error = None;
                            announce_due_at = Instant::now() + DISCOVERY_ANNOUNCE_INTERVAL;
                            refresh_discovered_peer_snapshot(
                                &discovered,
                                &event_tx,
                                &mut last_signature,
                                OffsetDateTime::now_utc(),
                            );
                            DiscoveryLoopAction::Continue
                        }
                    }
                    command = command_rx.recv() => {
                        let Some(command) = command else {
                            break;
                        };

                        match command {
                            DiscoveryCommand::SendTrustRequest { peer } => {
                                let request_id = Uuid::new_v4();
                                let target_ip = match peer.address.parse::<IpAddr>() {
                                    Ok(address) => address,
                                    Err(error) => {
                                        let _ = event_tx.send(RuntimeEvent::TrustRequestFailed {
                                            peer_device_id: peer.device_id.clone(),
                                            message: error.to_string(),
                                        });
                                        continue;
                                    }
                                };
                                pending_outgoing_trust_requests.retain(|_, pending| {
                                    pending.device_id != peer.device_id
                                        || pending.fingerprint != peer.fingerprint
                                });
                                match socket
                                    .send_trust_request(
                                        &discovery_config,
                                        request_id,
                                        SocketAddr::new(target_ip, DISCOVERY_PORT),
                                    )
                                    .await
                                {
                                    Ok(()) => {
                                        pending_outgoing_trust_requests.insert(request_id, peer);
                                        last_discovery_error = None;
                                        DiscoveryLoopAction::Continue
                                    }
                                    Err(error) => {
                                        let _ = event_tx.send(RuntimeEvent::TrustRequestFailed {
                                            peer_device_id: peer.device_id.clone(),
                                            message: error.to_string(),
                                        });
                                        report_discovery_error(
                                            &event_tx,
                                            &mut last_discovery_error,
                                            format!("发送信任请求失败: {error}"),
                                        );
                                        DiscoveryLoopAction::Continue
                                    }
                                }
                            }
                            DiscoveryCommand::SendTrustDecision { request_id, allow } => {
                                let Some(pending_request) =
                                    pending_incoming_trust_requests.remove(&request_id)
                                else {
                                    let _ = event_tx.send(RuntimeEvent::Status(
                                        "待处理的信任请求已失效。".to_string(),
                                    ));
                                    continue;
                                };

                                match socket
                                    .send_trust_decision(
                                        &discovery_config,
                                        request_id,
                                        allow,
                                        pending_request.source,
                                    )
                                    .await
                                {
                                    Ok(()) => {
                                        last_discovery_error = None;
                                        DiscoveryLoopAction::Continue
                                    }
                                    Err(error) => {
                                        let _ = event_tx.send(RuntimeEvent::Status(format!(
                                            "发送信任确认失败: {error}"
                                        )));
                                        report_discovery_error(
                                            &event_tx,
                                            &mut last_discovery_error,
                                            format!("发送信任确认失败: {error}"),
                                        );
                                        DiscoveryLoopAction::Continue
                                    }
                                }
                            }
                        }
                    }
                    recv_result = socket.recv_message(
                        &mut discovery_buffer,
                        &discovery_config.device_id,
                        &discovery_config.fingerprint,
                    ) => {
                        match recv_result {
                            Ok(Some(ReceivedDiscoveryMessage::Probe { source, .. })) => {
                                if let Err(error) = socket.reply_with_announce(&discovery_config, source).await {
                                    report_discovery_error(
                                        &event_tx,
                                        &mut last_discovery_error,
                                        format!("响应设备发现探测失败: {error}"),
                                    );
                                    DiscoveryLoopAction::ResetSocket
                                } else {
                                    last_discovery_error = None;
                                    DiscoveryLoopAction::Continue
                                }
                            }
                            Ok(Some(ReceivedDiscoveryMessage::Announce(peer))) => {
                                let now = OffsetDateTime::now_utc();
                                {
                                    let mut map = discovered.write();
                                    merge_announced_peers(&mut map, [peer], now);
                                    prune_stale_discovered_peers(&mut map, now);
                                }
                                emit_discovered_peer_snapshot(
                                    &discovered,
                                    &event_tx,
                                    &mut last_signature,
                                );
                                last_discovery_error = None;
                                DiscoveryLoopAction::Continue
                            }
                            Ok(Some(ReceivedDiscoveryMessage::TrustRequest { source, request_id, peer })) => {
                                let now = OffsetDateTime::now_utc();
                                {
                                    let mut map = discovered.write();
                                    merge_announced_peers(&mut map, [peer.clone()], now);
                                    prune_stale_discovered_peers(&mut map, now);
                                }
                                emit_discovered_peer_snapshot(
                                    &discovered,
                                    &event_tx,
                                    &mut last_signature,
                                );

                                if is_trusted_peer(&config_state.read(), &peer) {
                                    if let Err(error) = socket
                                        .send_trust_decision(
                                            &discovery_config,
                                            request_id,
                                            true,
                                            source,
                                        )
                                        .await
                                    {
                                        report_discovery_error(
                                            &event_tx,
                                            &mut last_discovery_error,
                                            format!("自动确认信任请求失败: {error}"),
                                        );
                                        DiscoveryLoopAction::Continue
                                    } else {
                                        last_discovery_error = None;
                                        DiscoveryLoopAction::Continue
                                    }
                                } else {
                                    pending_incoming_trust_requests.retain(|_, pending| {
                                        pending.peer.device_id != peer.device_id
                                            || pending.peer.fingerprint != peer.fingerprint
                                    });
                                    pending_incoming_trust_requests.insert(
                                        request_id,
                                        PendingIncomingTrustRequest { source, peer: peer.clone() },
                                    );
                                    let _ = event_tx.send(RuntimeEvent::TrustRequestReceived {
                                        request_id,
                                        peer,
                                    });
                                    last_discovery_error = None;
                                    DiscoveryLoopAction::Continue
                                }
                            }
                            Ok(Some(ReceivedDiscoveryMessage::TrustDecision { request_id, peer, accepted })) => {
                                match pending_outgoing_trust_requests.remove(&request_id) {
                                    Some(requested_peer) => {
                                        let now = OffsetDateTime::now_utc();
                                        {
                                            let mut map = discovered.write();
                                            merge_announced_peers(&mut map, [peer.clone()], now);
                                            prune_stale_discovered_peers(&mut map, now);
                                        }
                                        emit_discovered_peer_snapshot(
                                            &discovered,
                                            &event_tx,
                                            &mut last_signature,
                                        );

                                        if peer.device_id != requested_peer.device_id
                                            || peer.fingerprint != requested_peer.fingerprint
                                        {
                                            let _ = event_tx.send(RuntimeEvent::TrustRequestFailed {
                                                peer_device_id: requested_peer.device_id,
                                                message: "The trust response did not match the requested device identity."
                                                    .to_string(),
                                            });
                                            DiscoveryLoopAction::Continue
                                        } else {
                                            let _ = event_tx.send(RuntimeEvent::TrustRequestCompleted {
                                                peer,
                                                accepted,
                                            });
                                            last_discovery_error = None;
                                            DiscoveryLoopAction::Continue
                                        }
                                    }
                                    None => DiscoveryLoopAction::Continue,
                                }
                            }
                            Ok(None) => DiscoveryLoopAction::Continue,
                            Err(error) => {
                                report_discovery_error(
                                    &event_tx,
                                    &mut last_discovery_error,
                                    format!("接收设备发现数据失败: {error}"),
                                );
                                DiscoveryLoopAction::ResetSocket
                            }
                        }
                    }
                    _ = wakeup.notified() => {
                        DiscoveryLoopAction::RequestProbe
                    }
                };

                match action {
                    DiscoveryLoopAction::Continue => {}
                    DiscoveryLoopAction::ResetSocket => {
                        discovery_socket.take();
                        active_config.take();
                        announce_due_at = Instant::now() + DISCOVERY_RETRY_INTERVAL;
                    }
                    DiscoveryLoopAction::RequestProbe => {
                        probe_requested = true;
                    }
                }
            }
        }
    });
}

fn refresh_discovered_peer_snapshot(
    discovered: &Arc<RwLock<HashMap<String, DiscoveredPeer>>>,
    event_tx: &Sender<RuntimeEvent>,
    last_signature: &mut Vec<(String, String, String, u16, String)>,
    now: OffsetDateTime,
) {
    {
        let mut map = discovered.write();
        prune_stale_discovered_peers(&mut map, now);
    }
    emit_discovered_peer_snapshot(discovered, event_tx, last_signature);
}

fn emit_discovered_peer_snapshot(
    discovered: &Arc<RwLock<HashMap<String, DiscoveredPeer>>>,
    event_tx: &Sender<RuntimeEvent>,
    last_signature: &mut Vec<(String, String, String, u16, String)>,
) {
    let peers = {
        let map = discovered.read();
        sorted_discovered_peers(&map)
    };
    let signature = peer_list_signature(&peers);
    if signature != *last_signature {
        *last_signature = signature;
        let _ = event_tx.send(RuntimeEvent::PeerList(peers));
    }
}

fn report_discovery_error(
    event_tx: &Sender<RuntimeEvent>,
    last_discovery_error: &mut Option<String>,
    message: String,
) {
    if last_discovery_error.as_deref() != Some(message.as_str()) {
        *last_discovery_error = Some(message.clone());
        let _ = event_tx.send(RuntimeEvent::Status(message));
    }
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

pub(super) fn prune_stale_discovered_peers(
    discovered: &mut HashMap<String, DiscoveredPeer>,
    now: OffsetDateTime,
) {
    let grace_seconds = DISCOVERY_OFFLINE_GRACE.as_secs() as i64;
    discovered
        .retain(|_, peer| now.unix_timestamp() - peer.last_seen.unix_timestamp() <= grace_seconds);
}

pub(super) fn sorted_discovered_peers(
    discovered: &HashMap<String, DiscoveredPeer>,
) -> Vec<DiscoveredPeer> {
    let mut peers = discovered.values().cloned().collect::<Vec<_>>();
    peers.sort_by(|left, right| {
        left.device_name
            .cmp(&right.device_name)
            .then(left.device_id.cmp(&right.device_id))
    });
    peers
}
