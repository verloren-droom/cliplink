use std::collections::HashSet;

use super::*;
use crate::core::{
    model::{ClipboardItem, DiscoveredPeer, TrustedPeer},
    runtime::{FileTransferProgress, RuntimeEvent},
};
use time::OffsetDateTime;

impl AppController {
    pub(super) fn handle_runtime_event(&mut self, event: RuntimeEvent, outcome: &mut TickOutcome) {
        match event {
            RuntimeEvent::PeerList(peers) => self.handle_peer_list_event(peers, outcome),
            RuntimeEvent::TrustRequestReceived { request_id, peer } => {
                self.handle_trust_request_received_event(request_id, peer, outcome)
            }
            RuntimeEvent::TrustRequestCompleted { peer, accepted } => {
                self.handle_trust_request_completed_event(peer, accepted, outcome)
            }
            RuntimeEvent::TrustRequestFailed {
                peer_device_id,
                message,
            } => self.handle_trust_request_failed_event(peer_device_id, message, outcome),
            RuntimeEvent::TrustRevokedByPeer {
                peer_device_id,
                peer_device_name,
            } => self.handle_trust_revoked_by_peer_event(peer_device_id, peer_device_name, outcome),
            RuntimeEvent::RemoteShareStateChanged {
                peer_device_id,
                peer_device_name,
                share_local_history,
            } => self.handle_remote_share_state_changed_event(
                peer_device_id,
                peer_device_name,
                share_local_history,
                outcome,
            ),
            RuntimeEvent::InboundClipboard(item) => {
                self.handle_inbound_clipboard_event(item, outcome)
            }
            RuntimeEvent::RemoteHistoryItemsRemoved {
                peer_device_id,
                peer_device_name,
                item_ids,
            } => self.handle_remote_history_items_removed_event(
                peer_device_id,
                peer_device_name,
                item_ids,
                outcome,
            ),
            RuntimeEvent::HistorySnapshotReceived {
                peer_device_id,
                peer_device_name,
                items,
            } => self.handle_history_snapshot_received_event(
                peer_device_id,
                peer_device_name,
                items,
                outcome,
            ),
            RuntimeEvent::HistorySnapshotFailed {
                peer_device_id,
                message,
            } => self.handle_history_snapshot_failed_event(peer_device_id, message, outcome),
            RuntimeEvent::RemoteClipboardResolved(item) => {
                self.handle_remote_clipboard_resolved_event(item, outcome)
            }
            RuntimeEvent::RemoteClipboardFailed { item_id, message } => {
                self.handle_remote_clipboard_failed_event(item_id, message, outcome)
            }
            RuntimeEvent::RemoteFilesResolved(item) => {
                self.handle_remote_files_resolved_event(item, outcome)
            }
            RuntimeEvent::RemoteFilesFailed { item_id, message } => {
                self.handle_remote_files_failed_event(item_id, message, outcome)
            }
            RuntimeEvent::FileTransferProgress(progress) => {
                self.handle_file_transfer_progress_event(progress, outcome)
            }
            RuntimeEvent::Status(status) => self.handle_runtime_status_event(status, outcome),
        }
    }

    fn handle_peer_list_event(&mut self, peers: Vec<DiscoveredPeer>, outcome: &mut TickOutcome) {
        let reconcile_result = match self.reconcile_trusted_peers_with_discovered(&peers) {
            Ok(result) => Some(result),
            Err(error) => {
                self.set_status(format!("收敛设备信任状态失败: {error}"));
                outcome.status_changed = true;
                None
            }
        };
        let peers_changed = self.discovered != peers;
        for peer in &peers {
            self.device_last_seen
                .insert(peer.device_id.clone(), peer.last_seen);
        }
        self.discovered = peers;
        let online_ids = self
            .discovered
            .iter()
            .map(|peer| peer.device_id.clone())
            .collect::<HashSet<_>>();
        self.remote_share_enabled
            .retain(|device_id, _| online_ids.contains(device_id));
        self.requested_history_sync_peers
            .retain(|device_id| online_ids.contains(device_id));
        self.announced_share_state_peers
            .retain(|device_id| online_ids.contains(device_id));
        let removed_ids = self.remote_history.retain_online_devices(&online_ids);
        if !removed_ids.is_empty() {
            self.cleanup_remote_item_dirs(removed_ids);
            outcome.history_changed = true;
        }
        let mut runtime_resynced = false;
        if let Some(result) = reconcile_result {
            if result.devices_changed {
                outcome.devices_changed = true;
            }
            if result.history_changed {
                outcome.history_changed = true;
            }
            if result.devices_changed {
                match self.sync_runtime() {
                    Ok(()) => runtime_resynced = true,
                    Err(error) => {
                        self.set_status(format!("同步后台设备配置失败: {error}"));
                        outcome.status_changed = true;
                    }
                }
            }
        }
        if !runtime_resynced {
            self.sync_online_trusted_peer_history();
        }
        if peers_changed {
            outcome.devices_changed = true;
        }
    }

    fn handle_trust_request_received_event(
        &mut self,
        request_id: uuid::Uuid,
        peer: DiscoveredPeer,
        outcome: &mut TickOutcome,
    ) {
        self.pending_trust_requests.retain(|request| {
            request.request_id != request_id
                && !(request.device_id == peer.device_id && request.fingerprint == peer.fingerprint)
        });

        self.pending_trust_requests.push(PendingTrustRequest {
            request_id,
            device_id: peer.device_id.clone(),
            device_name: peer.device_name.clone(),
            secondary_text: format!("{}:{}", peer.address, peer.port),
            fingerprint: peer.fingerprint,
        });
        self.set_status(format!("{} 请求建立信任连接", peer.device_name));
        outcome.status_changed = true;
        outcome.devices_changed = true;
    }

    fn handle_trust_request_completed_event(
        &mut self,
        peer: DiscoveredPeer,
        accepted: bool,
        outcome: &mut TickOutcome,
    ) {
        if accepted {
            match self.apply_trusted_peer(TrustedPeer {
                device_id: peer.device_id.clone(),
                device_name: peer.device_name.clone(),
                fingerprint: peer.fingerprint.clone(),
            }) {
                Ok(true) | Ok(false) => {
                    self.set_status(format!("{} 已同意建立信任连接", peer.device_name));
                }
                Err(error) => {
                    self.set_status(format!(
                        "{} 已同意连接，但保存信任关系失败: {error}",
                        peer.device_name
                    ));
                }
            }
        } else {
            self.set_status(format!("{} 已拒绝本次连接请求", peer.device_name));
        }
        outcome.status_changed = true;
        outcome.devices_changed = true;
    }

    fn handle_trust_request_failed_event(
        &mut self,
        peer_device_id: String,
        message: String,
        outcome: &mut TickOutcome,
    ) {
        self.set_status(format!(
            "发送到设备 {peer_device_id} 的信任请求失败: {message}"
        ));
        outcome.status_changed = true;
    }

    fn handle_trust_revoked_by_peer_event(
        &mut self,
        peer_device_id: String,
        peer_device_name: String,
        outcome: &mut TickOutcome,
    ) {
        match self.revoke_device_trust_local(&peer_device_id) {
            Ok(true) => {
                self.set_status(format!(
                    "设备 {} 已移除对当前设备的信任，已同步更新本机",
                    peer_device_name
                ));
                outcome.devices_changed = true;
                outcome.history_changed = true;
            }
            Ok(false) => {}
            Err(error) => {
                self.set_status(format!("同步对端移除信任状态失败: {error}"));
                outcome.status_changed = true;
            }
        }
        outcome.status_changed = true;
    }

    fn handle_remote_share_state_changed_event(
        &mut self,
        peer_device_id: String,
        peer_device_name: String,
        share_local_history: bool,
        outcome: &mut TickOutcome,
    ) {
        self.requested_history_sync_peers.remove(&peer_device_id);
        if share_local_history {
            self.remote_share_enabled.insert(peer_device_id, true);
            self.sync_online_trusted_peer_history();
            self.set_status(format!("{} 已恢复共享本机剪切板历史", peer_device_name));
        } else {
            self.remote_share_enabled
                .insert(peer_device_id.clone(), false);
            outcome.history_changed |= self.clear_remote_history_for_peer(&peer_device_id);
            if self.sync_preferred_latest_to_clipboard().is_err() {
                self.set_status(format!(
                    "{} 已关闭共享本机剪切板历史，但更新系统剪切板失败",
                    peer_device_name
                ));
            } else {
                self.set_status(format!("{} 已关闭共享本机剪切板历史", peer_device_name));
            }
        }
        outcome.status_changed = true;
        outcome.devices_changed = true;
    }

    fn handle_inbound_clipboard_event(&mut self, item: ClipboardItem, outcome: &mut TickOutcome) {
        let inbound_source = item
            .source_device_name
            .clone()
            .unwrap_or_else(|| "远端设备".to_string());
        self.store_live_remote_item(item.clone());
        outcome.history_changed = true;

        if self.config.prefer_remote_latest_on_paste {
            match self.services.clipboard.write_item(&item) {
                Ok(()) => self.set_status(format!("已接收来自 {inbound_source} 的内容")),
                Err(error) => {
                    self.set_status(format!("更新系统剪切板失败: {error}"));
                }
            }
        } else {
            self.set_status(format!(
                "已接收来自 {inbound_source} 的内容，粘贴仍优先本机最新记录"
            ));
        }
        outcome.status_changed = true;
    }

    fn handle_history_snapshot_received_event(
        &mut self,
        peer_device_id: String,
        peer_device_name: String,
        items: Vec<ClipboardItem>,
        outcome: &mut TickOutcome,
    ) {
        self.replace_remote_history_snapshot(&peer_device_id, items);
        outcome.history_changed = true;
        self.set_status(format!("已同步 {} 的历史记录", peer_device_name));
        outcome.status_changed = true;
        if self.config.prefer_remote_latest_on_paste
            && self.sync_preferred_latest_to_clipboard().is_err()
        {
            self.set_status("同步远程历史后更新系统剪切板失败");
        }
    }

    fn handle_remote_history_items_removed_event(
        &mut self,
        peer_device_id: String,
        peer_device_name: String,
        item_ids: Vec<uuid::Uuid>,
        outcome: &mut TickOutcome,
    ) {
        let removed_ids = self
            .remote_history
            .remove_peer_items(&peer_device_id, &item_ids);
        if removed_ids.is_empty() {
            return;
        }

        self.cleanup_remote_item_dirs(removed_ids);
        outcome.history_changed = true;
        self.set_status(format!(
            "{} 已同步删除 {} 条历史记录",
            peer_device_name,
            item_ids.len()
        ));
        outcome.status_changed = true;
        if self.config.prefer_remote_latest_on_paste
            && self.sync_preferred_latest_to_clipboard().is_err()
        {
            self.set_status("同步远程历史删除后更新系统剪切板失败");
        }
    }

    fn handle_history_snapshot_failed_event(
        &mut self,
        peer_device_id: String,
        message: String,
        outcome: &mut TickOutcome,
    ) {
        self.set_status(format!(
            "拉取设备 {peer_device_id} 的远端历史失败: {message}"
        ));
        outcome.status_changed = true;
    }

    fn handle_remote_clipboard_resolved_event(
        &mut self,
        item: ClipboardItem,
        outcome: &mut TickOutcome,
    ) {
        let should_paste = self.pending_remote_activation == Some(item.id);
        self.pending_remote_activation = None;

        let mut item = match self.merge_remote_item_with_cached_state(item) {
            Ok(item) => item,
            Err(error) => {
                self.set_status(format!("远端文本合并失败: {error}"));
                outcome.status_changed = true;
                return;
            }
        };
        item.created_at = OffsetDateTime::now_utc();
        self.store_resolved_remote_item(&item);
        outcome.history_changed = true;

        match self.services.clipboard.write_item(&item) {
            Ok(()) => {
                self.set_status("远端内容已写入系统剪切板");
                outcome.status_changed = true;
                if should_paste {
                    outcome.paste_requested = true;
                }
            }
            Err(error) => {
                self.set_status(format!("远端内容已接收，但写入系统剪切板失败: {error}"));
                outcome.status_changed = true;
            }
        }
    }

    fn handle_remote_clipboard_failed_event(
        &mut self,
        item_id: uuid::Uuid,
        message: String,
        outcome: &mut TickOutcome,
    ) {
        if self.pending_remote_activation == Some(item_id) {
            self.pending_remote_activation = None;
        }
        self.set_status(format!("远端内容读取失败: {message}"));
        outcome.status_changed = true;
    }

    fn handle_remote_files_resolved_event(
        &mut self,
        item: ClipboardItem,
        outcome: &mut TickOutcome,
    ) {
        let should_paste = self.pending_remote_activation == Some(item.id);
        self.pending_remote_activation = None;
        self.active_transfer = None;
        outcome.transfer_changed = true;

        let mut item = match self.merge_remote_item_with_cached_state(item) {
            Ok(item) => item,
            Err(error) => {
                self.set_status(format!("远程文件合并失败: {error}"));
                outcome.status_changed = true;
                return;
            }
        };
        item.created_at = OffsetDateTime::now_utc();
        self.store_resolved_remote_item(&item);
        outcome.history_changed = true;

        match self.services.clipboard.write_item(&item) {
            Ok(()) => {
                self.set_status("远程文件已保存到本机并写入系统剪切板");
                outcome.status_changed = true;
                if should_paste {
                    outcome.paste_requested = true;
                }
            }
            Err(error) => {
                self.set_status(format!("远程文件已接收，但写入系统剪切板失败: {error}"));
                outcome.status_changed = true;
            }
        }
    }

    fn handle_remote_files_failed_event(
        &mut self,
        item_id: uuid::Uuid,
        message: String,
        outcome: &mut TickOutcome,
    ) {
        if self.pending_remote_activation == Some(item_id) {
            self.pending_remote_activation = None;
        }
        if self
            .active_transfer
            .as_ref()
            .is_some_and(|transfer| transfer.item_id == item_id)
        {
            self.active_transfer = None;
        }
        self.set_status(format!("远程文件传输失败: {message}"));
        outcome.status_changed = true;
        outcome.transfer_changed = true;
    }

    fn handle_file_transfer_progress_event(
        &mut self,
        progress: FileTransferProgress,
        outcome: &mut TickOutcome,
    ) {
        if self.apply_transfer_progress(progress) {
            outcome.transfer_changed = true;
        }
    }

    fn handle_runtime_status_event(&mut self, status: String, outcome: &mut TickOutcome) {
        self.set_status(status);
        outcome.status_changed = true;
    }

    fn store_live_remote_item(&mut self, item: ClipboardItem) {
        self.upsert_remote_history_item(item);
    }

    fn store_resolved_remote_item(&mut self, item: &ClipboardItem) {
        self.upsert_remote_history_item(item.clone());
        self.remote_history.touch_item(item.id, item.created_at);
    }
}
