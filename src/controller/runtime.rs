use std::{collections::HashSet, time::Instant};

use super::*;
use crate::{
    constants::limits::MAX_HISTORY_SNAPSHOT_ITEMS,
    core::{
        error::AppResult,
        model::ClipboardItem,
        runtime::{FileTransferProgress, RuntimeBridge, RuntimeCommand},
    },
};
use uuid::Uuid;

impl AppController {
    pub(crate) fn tick(&mut self) -> TickOutcome {
        let mut outcome = TickOutcome::default();

        while let Some(event) = self
            .services
            .runtime
            .as_ref()
            .and_then(RuntimeBridge::try_recv)
        {
            self.handle_runtime_event(event, &mut outcome);
        }

        let poll_interval = self.services.clipboard.recommended_poll_interval();
        if self.last_clipboard_poll.elapsed() >= poll_interval {
            self.last_clipboard_poll = Instant::now();

            match self
                .services
                .clipboard
                .poll(&self.config.device_id, &self.config.device_name)
            {
                Ok(Some(item)) => outcome.merge(self.handle_local_clipboard_item(item)),
                Ok(None) => {}
                Err(error) => {
                    self.set_status(format!("读取系统剪切板失败: {error}"));
                    outcome.status_changed = true;
                }
            }
        }

        outcome
    }

    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub(crate) fn submit_local_clipboard_item(&mut self, item: ClipboardItem) -> TickOutcome {
        self.handle_local_clipboard_item(item)
    }

    pub(super) fn handle_local_clipboard_item(&mut self, item: ClipboardItem) -> TickOutcome {
        let mut outcome = TickOutcome::default();
        match self
            .services
            .history_store
            .insert(&item, self.config.history_limit)
        {
            Ok(result) if result.inserted => {
                self.upsert_local_history_entry(Self::build_history_entry(&item));
                outcome.history_changed = true;

                if self.config.share_local_history {
                    if let Err(error) = self.broadcast_item(item.clone()) {
                        self.set_status(format!("本机内容已保存，但共享失败: {error}"));
                        outcome.status_changed = true;
                    }
                    if !result.pruned_ids.is_empty()
                        && self.broadcast_history_removals(result.pruned_ids).is_err()
                    {
                        self.set_status("本机内容已保存，但同步裁剪后的历史删除失败");
                        outcome.status_changed = true;
                    }
                }
            }
            Ok(_) => {}
            Err(error) => {
                self.set_status(format!("保存本机剪切板历史失败: {error}"));
                outcome.status_changed = true;
            }
        }
        outcome
    }

    pub(super) fn sync_online_trusted_peer_history(&mut self) {
        let Some(runtime) = self.services.runtime.as_ref() else {
            return;
        };
        if self.config.trusted_peers.is_empty() {
            return;
        }

        let trusted_fingerprints = self
            .config
            .trusted_peers
            .iter()
            .map(|peer| peer.fingerprint.as_str())
            .filter(|fingerprint| !fingerprint.is_empty())
            .collect::<HashSet<_>>();
        if trusted_fingerprints.is_empty() {
            return;
        }
        let mut should_announce_share_state = false;
        for peer in &self.discovered {
            if peer.fingerprint.is_empty()
                || !trusted_fingerprints.contains(peer.fingerprint.as_str())
            {
                continue;
            }
            if self
                .requested_history_sync_peers
                .insert(peer.device_id.clone())
            {
                runtime.send(RuntimeCommand::RequestHistorySnapshot {
                    peer_device_id: peer.device_id.clone(),
                    limit: self
                        .config
                        .history_limit
                        .clamp(1, MAX_HISTORY_SNAPSHOT_ITEMS),
                });
            }
            if self
                .announced_share_state_peers
                .insert(peer.device_id.clone())
            {
                should_announce_share_state = true;
            }
        }

        if should_announce_share_state {
            runtime.send(RuntimeCommand::NotifyShareState {
                share_local_history: self.config.share_local_history,
            });
        }
    }

    fn runtime_should_run(&self) -> bool {
        self.config.discovery_enabled
            || !self.config.trusted_peers.is_empty()
            || (self.preferences_visible
                && self
                    .services
                    .runtime_policy
                    .keep_discovery_running_without_trusted_peers)
    }

    pub(super) fn ensure_runtime_started(&mut self) -> AppResult<()> {
        if self.services.runtime.is_none() {
            self.services.runtime = Some(RuntimeBridge::start(
                self.services.paths.clone(),
                self.config.clone(),
                self.services.local_data_cipher.clone(),
                self.services.history_store_profile,
            )?);
        }
        Ok(())
    }

    fn stop_runtime(&mut self) {
        if self.services.runtime.take().is_some() {
            self.discovered.clear();
            self.pending_trust_requests.clear();
            self.requested_history_sync_peers.clear();
            self.announced_share_state_peers.clear();
            self.pending_remote_activation = None;
            self.active_transfer = None;
            self.remote_share_enabled.clear();
            self.clear_all_remote_history();
        }
    }

    pub(super) fn sync_runtime(&mut self) -> AppResult<()> {
        if !self.runtime_should_run() {
            self.stop_runtime();
            return Ok(());
        }

        self.ensure_runtime_started()?;
        if let Some(runtime) = self.services.runtime.as_ref() {
            runtime.send(RuntimeCommand::ReplaceConfig(self.config.clone()));
        }
        self.sync_online_trusted_peer_history();
        Ok(())
    }

    pub(super) fn broadcast_item(&mut self, item: ClipboardItem) -> AppResult<()> {
        if self.services.runtime.is_none() && !self.config.trusted_peers.is_empty() {
            self.sync_runtime()?;
        }

        if let Some(runtime) = self.services.runtime.as_ref() {
            runtime.send(RuntimeCommand::Broadcast(item));
        }
        Ok(())
    }

    pub(super) fn broadcast_history_removals(&mut self, item_ids: Vec<Uuid>) -> AppResult<()> {
        if item_ids.is_empty() {
            return Ok(());
        }

        if self.services.runtime.is_none() && !self.config.trusted_peers.is_empty() {
            self.sync_runtime()?;
        }

        if let Some(runtime) = self.services.runtime.as_ref() {
            runtime.send(RuntimeCommand::BroadcastHistoryRemovals { item_ids });
        }
        Ok(())
    }

    pub(super) fn notify_share_state(&mut self, share_local_history: bool) -> AppResult<()> {
        if self.services.runtime.is_none() && !self.config.trusted_peers.is_empty() {
            self.sync_runtime()?;
        }

        if let Some(runtime) = self.services.runtime.as_ref() {
            runtime.send(RuntimeCommand::NotifyShareState {
                share_local_history,
            });
        }
        Ok(())
    }

    fn preferred_latest_item(&self) -> AppResult<Option<ClipboardItem>> {
        let latest_local = self
            .local_history
            .first()
            .map(|item| item.id)
            .map(|id| self.load_local_item(id))
            .transpose()?
            .flatten();

        let latest_remote = self.remote_history.latest_cached_item();

        match (
            latest_local,
            latest_remote,
            self.config.prefer_remote_latest_on_paste,
        ) {
            (Some(local), Some(remote), true) => {
                if remote.created_at > local.created_at {
                    Ok(Some(remote))
                } else {
                    Ok(Some(local))
                }
            }
            (Some(local), Some(_remote), false) => Ok(Some(local)),
            (Some(local), None, _) => Ok(Some(local)),
            (None, Some(remote), true) => Ok(Some(remote)),
            (None, Some(_remote), false) => Ok(None),
            (None, None, _) => Ok(None),
        }
    }

    pub(super) fn sync_preferred_latest_to_clipboard(
        &mut self,
    ) -> AppResult<Option<ClipboardItem>> {
        let Some(item) = self.preferred_latest_item()? else {
            return Ok(None);
        };

        self.services.clipboard.write_item(&item)?;
        Ok(Some(item))
    }

    pub(super) fn apply_transfer_progress(&mut self, progress: FileTransferProgress) -> bool {
        let changed = self.active_transfer.as_ref().is_none_or(|current| {
            current.item_id != progress.item_id
                || current.bytes_done != progress.bytes_done
                || current.bytes_total != progress.bytes_total
                || current.summary != progress.summary
                || current.source_device_name != progress.source_device_name
        });

        if changed {
            self.active_transfer = Some(ActiveTransferState {
                item_id: progress.item_id,
                source_device_name: progress.source_device_name,
                summary: progress.summary,
                bytes_done: progress.bytes_done,
                bytes_total: progress.bytes_total,
            });
        }

        changed
    }

    pub(super) fn set_status(&mut self, status: impl Into<String>) {
        self.status = Some(status.into());
    }
}
