use super::*;
use std::collections::BTreeSet;

use crate::core::{
    error::{AppError, AppResult},
    model::{DiscoveredPeer, TrustedPeer},
};

#[derive(Default)]
pub(super) struct TrustedPeerDiscoveryReconcileResult {
    pub(super) devices_changed: bool,
    pub(super) history_changed: bool,
}

#[derive(Default, Debug, PartialEq, Eq)]
struct TrustedPeerFingerprintReconcileResult {
    changed: bool,
    removed_device_ids: Vec<String>,
}

impl AppController {
    pub(crate) fn request_device_trust(&mut self, device_id: &str) -> AppResult<bool> {
        let Some(peer) = self
            .discovered
            .iter()
            .find(|peer| peer.device_id == device_id)
            .cloned()
        else {
            self.set_status("仅支持向当前在线设备发起信任请求");
            return Ok(false);
        };

        if self
            .config
            .trusted_peers
            .iter()
            .any(|trusted| trusted.fingerprint == peer.fingerprint)
        {
            let updated = self.apply_trusted_peer(TrustedPeer {
                device_id: peer.device_id.clone(),
                device_name: peer.device_name.clone(),
                fingerprint: peer.fingerprint.clone(),
            })?;
            self.set_status(if updated {
                format!("已更新设备 {} 的信任标识", peer.device_name)
            } else {
                format!("设备 {} 已在信任列表中", peer.device_name)
            });
            return Ok(false);
        }

        if self.config.trusted_peers.iter().any(|trusted| {
            trusted.device_id == peer.device_id && trusted.fingerprint == peer.fingerprint
        }) {
            self.set_status(format!("设备 {} 已在信任列表中", peer.device_name));
            return Ok(false);
        }

        self.ensure_runtime_started()?;
        let Some(runtime) = self.services.runtime.as_ref() else {
            return Err(AppError::Network(
                "Runtime is not available for trust negotiation.".to_string(),
            ));
        };

        runtime.send(crate::core::runtime::RuntimeCommand::RequestTrust {
            peer_device_id: peer.device_id.clone(),
        });
        self.set_status(format!(
            "已向 {} 发送信任请求，等待对方确认",
            peer.device_name
        ));
        Ok(true)
    }

    pub(crate) fn request_device_trust_many(&mut self, device_ids: &[String]) -> AppResult<usize> {
        let mut requested = 0usize;

        for device_id in device_ids
            .iter()
            .map(|device_id| device_id.trim())
            .filter(|device_id| !device_id.is_empty())
            .map(str::to_string)
            .collect::<BTreeSet<_>>()
        {
            if self.request_device_trust(device_id.as_str())? {
                requested += 1;
            }
        }

        if requested > 0 {
            self.set_status(if requested == 1 {
                "已发送 1 个信任请求，等待对方确认".to_string()
            } else {
                format!("已发送 {requested} 个信任请求，等待对方确认")
            });
        }

        Ok(requested)
    }

    pub(crate) fn pending_trust_request(&self) -> Option<PendingTrustRequest> {
        self.pending_trust_requests.first().cloned()
    }

    pub(crate) fn respond_to_trust_request(
        &mut self,
        request_id: uuid::Uuid,
        allow: bool,
    ) -> AppResult<bool> {
        let Some(index) = self
            .pending_trust_requests
            .iter()
            .position(|request| request.request_id == request_id)
        else {
            self.set_status("连接请求已失效");
            return Ok(false);
        };

        let request = self.pending_trust_requests.remove(index);
        if allow {
            let _ = self.apply_trusted_peer(TrustedPeer {
                device_id: request.device_id.clone(),
                device_name: request.device_name.clone(),
                fingerprint: request.fingerprint.clone(),
            })?;
        }

        if let Some(runtime) = self.services.runtime.as_ref() {
            runtime.send(crate::core::runtime::RuntimeCommand::ResolveTrustRequest {
                request_id,
                allow,
            });
        }

        if allow {
            self.set_status(format!("已同意 {} 的连接请求", request.device_name));
        } else {
            self.set_status(format!("已拒绝 {} 的连接请求", request.device_name));
        }
        Ok(true)
    }

    pub(crate) fn apply_trusted_peer(&mut self, next_peer: TrustedPeer) -> AppResult<bool> {
        let fingerprint_reconcile =
            reconcile_trusted_peer_by_fingerprint(&mut self.config.trusted_peers, &next_peer);
        let mut updated = fingerprint_reconcile.changed;
        if !updated {
            if let Some(existing) = self
                .config
                .trusted_peers
                .iter_mut()
                .find(|existing| existing.device_id == next_peer.device_id)
            {
                if *existing != next_peer {
                    *existing = next_peer.clone();
                    updated = true;
                }
            } else {
                self.config.trusted_peers.push(next_peer.clone());
                updated = true;
            }
        }

        if !updated {
            return Ok(false);
        }

        let _ = self.clear_removed_trusted_device_state(
            fingerprint_reconcile.removed_device_ids.as_slice(),
        );
        self.config
            .trusted_peers
            .sort_by(|left, right| left.device_name.cmp(&right.device_name));
        self.services.config_store.save(&self.config)?;
        self.sync_runtime()?;
        self.requested_history_sync_peers
            .remove(next_peer.device_id.as_str());
        self.remote_share_enabled
            .remove(next_peer.device_id.as_str());
        self.announced_share_state_peers
            .remove(next_peer.device_id.as_str());
        self.sync_online_trusted_peer_history();
        Ok(true)
    }

    pub(super) fn reconcile_trusted_peers_with_discovered(
        &mut self,
        discovered_peers: &[DiscoveredPeer],
    ) -> AppResult<TrustedPeerDiscoveryReconcileResult> {
        let mut result = TrustedPeerDiscoveryReconcileResult::default();
        let mut removed_device_ids = BTreeSet::new();

        for peer in discovered_peers {
            let reconcile =
                reconcile_trusted_peer_by_fingerprint(&mut self.config.trusted_peers, peer);
            if !reconcile.changed {
                continue;
            }

            result.devices_changed = true;
            removed_device_ids.extend(reconcile.removed_device_ids);
        }

        if !removed_device_ids.is_empty() {
            let removed_device_ids = removed_device_ids.into_iter().collect::<Vec<_>>();
            if self.clear_removed_trusted_device_state(&removed_device_ids) {
                result.history_changed = true;
            }
        }

        if result.devices_changed {
            self.config
                .trusted_peers
                .sort_by(|left, right| left.device_name.cmp(&right.device_name));
            self.services.config_store.save(&self.config)?;
        }

        Ok(result)
    }

    pub(crate) fn revoke_device_trust(&mut self, device_id: &str) -> AppResult<bool> {
        let _ = self.ensure_runtime_started();
        if let Some(runtime) = self.services.runtime.as_ref() {
            runtime.send(crate::core::runtime::RuntimeCommand::NotifyTrustRevoked {
                peer_device_id: device_id.to_string(),
            });
        }

        let Some(removed_name) = self.remove_trusted_peer(device_id)? else {
            return Ok(false);
        };
        self.set_status(format!("已移除信任设备 {}", removed_name));
        Ok(true)
    }

    pub(crate) fn revoke_device_trust_many(&mut self, device_ids: &[String]) -> AppResult<usize> {
        let mut revoked = 0usize;

        for device_id in device_ids
            .iter()
            .map(|device_id| device_id.trim())
            .filter(|device_id| !device_id.is_empty())
            .map(str::to_string)
            .collect::<BTreeSet<_>>()
        {
            if self.revoke_device_trust(device_id.as_str())? {
                revoked += 1;
            }
        }

        if revoked > 0 {
            self.set_status(if revoked == 1 {
                "已移除 1 个信任设备".to_string()
            } else {
                format!("已移除 {revoked} 个信任设备")
            });
        }

        Ok(revoked)
    }

    pub(super) fn revoke_device_trust_local(&mut self, device_id: &str) -> AppResult<bool> {
        Ok(self.remove_trusted_peer(device_id)?.is_some())
    }

    fn remove_trusted_peer(&mut self, device_id: &str) -> AppResult<Option<String>> {
        let removed_name = self
            .config
            .trusted_peers
            .iter()
            .find(|peer| peer.device_id == device_id)
            .map(|peer| peer.device_name.clone());
        let before = self.config.trusted_peers.len();
        self.config
            .trusted_peers
            .retain(|peer| peer.device_id != device_id);
        if self.config.trusted_peers.len() == before {
            return Ok(None);
        }

        self.services.config_store.save(&self.config)?;
        self.sync_runtime()?;
        let _ = self.clear_removed_trusted_device_state(&[device_id.to_string()]);
        Ok(removed_name)
    }

    fn clear_removed_trusted_device_state(&mut self, device_ids: &[String]) -> bool {
        let device_ids = device_ids
            .iter()
            .map(|device_id| device_id.trim())
            .filter(|device_id| !device_id.is_empty())
            .collect::<BTreeSet<_>>();

        if device_ids.is_empty() {
            return false;
        }

        for device_id in &device_ids {
            self.requested_history_sync_peers.remove(*device_id);
            self.announced_share_state_peers.remove(*device_id);
            self.remote_share_enabled.remove(*device_id);
            self.device_last_seen.remove(*device_id);
        }

        self.clear_remote_history_for_peers(device_ids)
    }
}

fn reconcile_trusted_peer_by_fingerprint<T>(
    trusted_peers: &mut Vec<TrustedPeer>,
    peer: &T,
) -> TrustedPeerFingerprintReconcileResult
where
    T: TrustedPeerLike,
{
    let matching = trusted_peers
        .iter()
        .enumerate()
        .filter(|(_, trusted)| trusted.fingerprint == peer.fingerprint())
        .map(|(index, trusted)| (index, trusted.device_id.clone()))
        .collect::<Vec<_>>();

    if matching.is_empty() {
        return TrustedPeerFingerprintReconcileResult::default();
    }

    let canonical_index = matching
        .iter()
        .find(|(_, device_id)| device_id == peer.device_id())
        .map(|(index, _)| *index)
        .unwrap_or(matching[0].0);

    let mut canonical = trusted_peers[canonical_index].clone();
    let mut changed = false;
    let mut removed_device_ids = Vec::new();
    if canonical.device_id != peer.device_id() {
        removed_device_ids.push(canonical.device_id.clone());
        canonical.device_id = peer.device_id().to_string();
        changed = true;
    }
    if canonical.device_name != peer.device_name() {
        canonical.device_name = peer.device_name().to_string();
        changed = true;
    }
    if canonical.fingerprint != peer.fingerprint() {
        canonical.fingerprint = peer.fingerprint().to_string();
        changed = true;
    }

    let mut next = Vec::with_capacity(trusted_peers.len());
    for (index, trusted) in trusted_peers.drain(..).enumerate() {
        if trusted.fingerprint != peer.fingerprint() {
            next.push(trusted);
            continue;
        }
        if index == canonical_index {
            continue;
        }
        if trusted.device_id != peer.device_id() {
            removed_device_ids.push(trusted.device_id);
        }
        changed = true;
    }
    next.push(canonical);
    *trusted_peers = next;

    removed_device_ids.sort();
    removed_device_ids.dedup();

    TrustedPeerFingerprintReconcileResult {
        changed,
        removed_device_ids,
    }
}

trait TrustedPeerLike {
    fn device_id(&self) -> &str;
    fn device_name(&self) -> &str;
    fn fingerprint(&self) -> &str;
}

impl TrustedPeerLike for TrustedPeer {
    fn device_id(&self) -> &str {
        &self.device_id
    }

    fn device_name(&self) -> &str {
        &self.device_name
    }

    fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}

impl TrustedPeerLike for DiscoveredPeer {
    fn device_id(&self) -> &str {
        &self.device_id
    }

    fn device_name(&self) -> &str {
        &self.device_name
    }

    fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trusted_peer(device_id: &str, device_name: &str, fingerprint: &str) -> TrustedPeer {
        TrustedPeer {
            device_id: device_id.to_string(),
            device_name: device_name.to_string(),
            fingerprint: fingerprint.to_string(),
        }
    }

    fn discovered_peer(device_id: &str, device_name: &str, fingerprint: &str) -> DiscoveredPeer {
        DiscoveredPeer {
            device_id: device_id.to_string(),
            device_name: device_name.to_string(),
            host: "peer.local".to_string(),
            address: "192.168.1.10".to_string(),
            port: 27_841,
            fingerprint: fingerprint.to_string(),
            last_seen: time::OffsetDateTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn reconcile_trusted_peer_by_fingerprint_updates_device_id_and_drops_duplicates() {
        let mut trusted = vec![
            trusted_peer("old-id", "HUAWEI NOH-AN01", "fp-1"),
            trusted_peer("older-id", "HUAWEI NOH-AN01", "fp-1"),
        ];

        let result = reconcile_trusted_peer_by_fingerprint(
            &mut trusted,
            &discovered_peer("new-id", "HUAWEI NOH-AN01", "fp-1"),
        );

        assert!(result.changed);
        assert_eq!(
            result.removed_device_ids,
            vec!["old-id".to_string(), "older-id".to_string()]
        );
        assert_eq!(trusted.len(), 1);
        assert_eq!(trusted[0].device_id, "new-id");
        assert_eq!(trusted[0].fingerprint, "fp-1");
    }
}
