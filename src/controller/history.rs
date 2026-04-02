use std::{
    collections::{BTreeMap, BTreeSet},
    io::ErrorKind,
};

use super::*;
use crate::core::{
    error::{AppError, AppResult},
    model::{ClipboardItem, ClipboardKind, ClipboardPayload},
    runtime::RuntimeCommand,
};
use time::OffsetDateTime;
use uuid::Uuid;

impl AppController {
    #[cfg_attr(target_os = "android", allow(dead_code))]
    pub(crate) fn report_status(&mut self, status: impl Into<String>) {
        self.set_status(status);
    }

    #[cfg_attr(target_os = "android", allow(dead_code))]
    pub(crate) fn status_text(&self) -> Option<String> {
        self.status.clone()
    }

    #[cfg_attr(target_os = "android", allow(dead_code))]
    pub(crate) fn transfer_progress_snapshot(&self) -> Option<TransferProgressSnapshot> {
        self.active_transfer
            .as_ref()
            .map(|progress| TransferProgressSnapshot {
                item_id: progress.item_id,
                label: format!("正在接收来自 {} 的文件", progress.source_device_name),
                detail: format!(
                    "{} · {} / {}",
                    progress.summary,
                    Self::format_bytes(progress.bytes_done),
                    Self::format_bytes(progress.bytes_total.max(progress.bytes_done))
                ),
                fraction: if progress.bytes_total == 0 {
                    1.0
                } else {
                    (progress.bytes_done as f64 / progress.bytes_total as f64).clamp(0.0, 1.0)
                },
                source_device_name: progress.source_device_name.clone(),
                summary: progress.summary.clone(),
                bytes_done: progress.bytes_done,
                bytes_total: progress.bytes_total,
            })
    }

    pub(crate) fn history_scope_options(&self) -> Vec<HistoryScopeOption> {
        let mut remote_sources = BTreeMap::<String, String>::new();

        for trusted_peer in &self.config.trusted_peers {
            let label = trusted_peer.device_name.trim();
            remote_sources
                .entry(trusted_peer.device_id.clone())
                .or_insert_with(|| {
                    if label.is_empty() {
                        trusted_peer.device_id.clone()
                    } else {
                        label.to_string()
                    }
                });
        }

        for entry in self.remote_history.entries() {
            let Some(device_id) = entry.source_device_id.as_ref() else {
                continue;
            };
            let label = entry.source_tooltip.trim();
            remote_sources.entry(device_id.clone()).or_insert_with(|| {
                if label.is_empty() {
                    device_id.clone()
                } else {
                    label.to_string()
                }
            });
        }

        let mut remote_options = remote_sources
            .into_iter()
            .map(|(device_id, label)| HistoryScopeOption {
                key: HistoryScope::Device(device_id).key(),
                label,
            })
            .collect::<Vec<_>>();
        remote_options.sort_by(|left, right| {
            left.label
                .cmp(&right.label)
                .then_with(|| left.key.cmp(&right.key))
        });

        let mut options = vec![
            HistoryScopeOption {
                key: HistoryScope::All.key(),
                label: "全部".to_string(),
            },
            HistoryScopeOption {
                key: HistoryScope::Local.key(),
                label: "本机".to_string(),
            },
        ];
        options.extend(remote_options);
        options
    }

    pub(crate) fn normalize_history_scope(&self, scope: HistoryScope) -> HistoryScope {
        let normalized_key = scope.key();
        if self
            .history_scope_options()
            .iter()
            .any(|option| option.key == normalized_key)
        {
            scope
        } else {
            HistoryScope::All
        }
    }

    pub(crate) fn history_rows_with_scope(
        &self,
        query: &str,
        scope: HistoryScope,
    ) -> Vec<HistoryRow> {
        let scope = self.normalize_history_scope(scope);
        let query = query.trim().to_lowercase();
        self.sorted_history_entries()
            .into_iter()
            .filter(|item| match scope {
                HistoryScope::All => true,
                HistoryScope::Local => !item.is_remote,
                HistoryScope::Device(ref device_id) => {
                    item.is_remote && item.source_device_id.as_deref() == Some(device_id.as_str())
                }
            })
            .filter(|item| query.is_empty() || item.search_blob.contains(query.as_str()))
            .map(Self::history_entry_to_row)
            .collect()
    }

    #[cfg_attr(target_os = "android", allow(dead_code))]
    pub(crate) fn load_item(&self, id: Uuid) -> AppResult<Option<ClipboardItem>> {
        if let Some(item) = self.load_local_item(id)? {
            return Ok(Some(item));
        }
        if let Some(item) = self.remote_history.cached_item(id) {
            return Ok(Some(item));
        }
        Ok(self
            .remote_history
            .item(id)
            .map(|item| item.to_placeholder_item()))
    }

    #[cfg_attr(target_os = "android", allow(dead_code))]
    pub(crate) fn recent_menu_entries(&self, limit: usize) -> Vec<(Uuid, String)> {
        self.sorted_history_entries()
            .into_iter()
            .take(limit)
            .map(|item| {
                (
                    item.id,
                    format!("[{}] {}", item.source_badge, item.summary_text),
                )
            })
            .collect()
    }

    pub(crate) fn clear_history_with_options(&mut self, include_pinned: bool) -> AppResult<()> {
        let delete_ids = self
            .local_history
            .iter()
            .filter(|item| include_pinned || !item.is_pinned)
            .map(|item| item.id)
            .collect::<Vec<_>>();

        for id in &delete_ids {
            self.services.history_store.delete_by_id(*id)?;
        }

        self.reload_local_history_cache()?;
        if self.config.share_local_history {
            self.broadcast_history_removals(delete_ids.clone())?;
        }
        if delete_ids.is_empty() {
            self.set_status(if include_pinned {
                "没有可清理的本机历史"
            } else {
                "没有可清理的本机未锁定历史"
            });
        } else if include_pinned {
            self.set_status("已清空本机剪切板历史，远程项已保留");
        } else if self.local_history.is_empty() {
            self.set_status("已清空本机剪切板历史");
        } else {
            self.set_status("已清空本机未锁定历史，远程项已保留");
        }
        Ok(())
    }

    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub(crate) fn delete_history_item(&mut self, id: Uuid) -> AppResult<bool> {
        self.delete_history_items(&[id]).map(|count| count > 0)
    }

    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub(crate) fn toggle_history_item_pin(&mut self, id: Uuid) -> AppResult<Option<bool>> {
        let Some(mut item) = self.load_local_item(id)? else {
            if self.remote_history.contains(id) {
                self.set_status("远程历史为只读，无法锁定");
            }
            return Ok(None);
        };

        item.is_pinned = !item.is_pinned;
        self.services.history_store.update_item(&item)?;
        self.upsert_local_history_entry(Self::build_history_entry(&item));
        self.set_status(if item.is_pinned {
            "已锁定该历史条目"
        } else {
            "已取消锁定该历史条目"
        });
        Ok(Some(item.is_pinned))
    }

    pub(crate) fn delete_history_items(&mut self, ids: &[Uuid]) -> AppResult<usize> {
        let mut deleted_ids = Vec::new();
        let mut skipped_remote = false;

        for id in ids.iter().copied().collect::<BTreeSet<_>>() {
            if self.remote_history.contains(id) {
                skipped_remote = true;
                continue;
            }
            if self.services.history_store.delete_by_id(id)? {
                deleted_ids.push(id);
            }
        }

        if deleted_ids.is_empty() {
            self.set_status(if skipped_remote {
                "远程历史为只读，无法删除"
            } else {
                "未找到可删除的本机历史"
            });
            return Ok(0);
        }

        self.reload_local_history_cache()?;
        if self.config.share_local_history {
            self.broadcast_history_removals(deleted_ids.clone())?;
        }

        self.set_status(match (deleted_ids.len(), skipped_remote) {
            (1, false) => "已删除剪切板历史".to_string(),
            (1, true) => "已删除 1 条本机历史，远程项已跳过".to_string(),
            (count, false) => format!("已删除 {count} 条剪切板历史"),
            (count, true) => format!("已删除 {count} 条本机历史，远程项已跳过"),
        });
        Ok(deleted_ids.len())
    }

    pub(crate) fn set_history_items_pinned(
        &mut self,
        ids: &[Uuid],
        pinned: bool,
    ) -> AppResult<usize> {
        let mut changed = 0usize;
        let mut skipped_remote = false;

        for id in ids.iter().copied().collect::<BTreeSet<_>>() {
            let Some(mut item) = self.load_local_item(id)? else {
                if self.remote_history.contains(id) {
                    skipped_remote = true;
                }
                continue;
            };

            if item.is_pinned == pinned {
                continue;
            }

            item.is_pinned = pinned;
            self.services.history_store.update_item(&item)?;
            changed += 1;
        }

        if changed == 0 {
            self.set_status(if skipped_remote {
                "远程历史为只读，无法修改锁定状态"
            } else if pinned {
                "选中条目已全部处于锁定状态"
            } else {
                "选中条目已全部处于未锁定状态"
            });
            return Ok(0);
        }

        self.reload_local_history_cache()?;
        self.set_status(match (changed, pinned, skipped_remote) {
            (1, true, false) => "已锁定 1 条历史".to_string(),
            (1, false, false) => "已取消锁定 1 条历史".to_string(),
            (count, true, false) => format!("已锁定 {count} 条历史"),
            (count, false, false) => format!("已取消锁定 {count} 条历史"),
            (1, true, true) => "已锁定 1 条本机历史，远程项已跳过".to_string(),
            (1, false, true) => "已取消锁定 1 条本机历史，远程项已跳过".to_string(),
            (count, true, true) => format!("已锁定 {count} 条本机历史，远程项已跳过"),
            (count, false, true) => format!("已取消锁定 {count} 条本机历史，远程项已跳过"),
        });
        Ok(changed)
    }

    pub(crate) fn copy_item(&mut self, id: Uuid) -> AppResult<HistoryActivation> {
        if let Some(mut item) = self.load_local_item(id)? {
            item.created_at = OffsetDateTime::now_utc();
            item = self.merge_local_item_with_existing_state(item)?;
            self.services.history_store.update_item(&item)?;
            self.upsert_local_history_entry(Self::build_history_entry(&item));
            self.services.clipboard.write_item(&item)?;
            self.set_status("已将历史记录写回系统剪切板");
            if self.config.share_local_history {
                if let Err(error) = self.broadcast_item(item) {
                    self.set_status(format!("已将历史记录写回系统剪切板，但共享失败: {error}"));
                }
            }
            return Ok(HistoryActivation::ClipboardReady);
        }

        if let Some(item) = self.remote_history.cached_item(id) {
            return self.activate_cached_remote_item(item);
        }

        let Some(remote_item) = self.remote_history.item(id).cloned() else {
            return Ok(HistoryActivation::Noop);
        };
        let Some(peer_device_id) = remote_item.source_device_id.clone() else {
            return Err(AppError::Network(
                "Remote history item is missing a source device identifier.".to_string(),
            ));
        };
        if self.services.runtime.is_none() {
            return Err(AppError::Network(
                "Remote transfer service is not running.".to_string(),
            ));
        }

        let activated_at = OffsetDateTime::now_utc();
        self.remote_history.touch_item(id, activated_at);
        self.pending_remote_activation = Some(id);

        match remote_item.history_entry.kind {
            ClipboardKind::Text => {
                let Some(runtime) = self.services.runtime.as_ref() else {
                    return Err(AppError::Network(
                        "Remote transfer service is not running.".to_string(),
                    ));
                };
                runtime.send(RuntimeCommand::FetchRemoteClipboardItem {
                    peer_device_id,
                    item_id: id,
                });
                self.set_status("正在读取远端文本…");
            }
            ClipboardKind::Files => {
                self.active_transfer = Some(ActiveTransferState {
                    item_id: id,
                    source_device_name: remote_item.history_entry.source_tooltip.clone(),
                    summary: remote_item.history_entry.summary_text.clone(),
                    bytes_done: 0,
                    bytes_total: remote_item.total_size_bytes,
                });
                let Some(runtime) = self.services.runtime.as_ref() else {
                    return Err(AppError::Network(
                        "Remote transfer service is not running.".to_string(),
                    ));
                };
                runtime.send(RuntimeCommand::FetchRemoteFiles(
                    remote_item.to_placeholder_item(),
                ));
                self.set_status("正在传输远程文件…");
            }
        }

        Ok(HistoryActivation::PendingTransfer)
    }

    pub(super) fn reload_local_history_cache(&mut self) -> AppResult<()> {
        self.local_history = self
            .services
            .history_store
            .recent(self.config.history_limit)?
            .into_iter()
            .map(|item| Self::build_history_entry(&item))
            .collect();
        Ok(())
    }

    fn sorted_history_entries(&self) -> Vec<&HistoryEntry> {
        let mut history = Vec::with_capacity(self.local_history.len() + self.remote_history.len());
        history.extend(self.local_history.iter());
        history.extend(self.remote_history.entries());
        history.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        history
    }

    pub(super) fn load_local_item(&self, id: Uuid) -> AppResult<Option<ClipboardItem>> {
        self.services.history_store.find_by_id(id)
    }

    fn activate_cached_remote_item(
        &mut self,
        mut item: ClipboardItem,
    ) -> AppResult<HistoryActivation> {
        item.created_at = OffsetDateTime::now_utc();
        item = self.merge_remote_item_with_cached_state(item)?;
        self.remote_history.cache_item(item.clone());
        self.remote_history.touch_item(item.id, item.created_at);
        self.services.clipboard.write_item(&item)?;
        self.set_status("已将远端历史写回系统剪切板");
        Ok(HistoryActivation::ClipboardReady)
    }

    pub(super) fn merge_local_item_with_existing_state(
        &self,
        item: ClipboardItem,
    ) -> AppResult<ClipboardItem> {
        let existing = self.load_local_item(item.id)?;
        Ok(Self::merge_item_with_existing_state(item, existing))
    }

    pub(super) fn merge_remote_item_with_cached_state(
        &self,
        item: ClipboardItem,
    ) -> AppResult<ClipboardItem> {
        let item_id = item.id;
        Ok(Self::merge_item_with_existing_state(
            item,
            self.remote_history.cached_item(item_id),
        ))
    }

    fn merge_item_with_existing_state(
        mut item: ClipboardItem,
        existing: Option<ClipboardItem>,
    ) -> ClipboardItem {
        let Some(existing) = existing else {
            return item;
        };

        if existing.created_at > item.created_at {
            item.created_at = existing.created_at;
        }
        item.is_pinned = existing.is_pinned;

        if let (ClipboardPayload::Files(files), ClipboardPayload::Files(existing_files)) =
            (&mut item.payload, &existing.payload)
        {
            let existing_by_path = existing_files
                .iter()
                .map(|file| (file.relative_path.as_str(), file))
                .collect::<BTreeMap<_, _>>();
            for file in files {
                let Some(existing_file) = existing_by_path.get(file.relative_path.as_str()) else {
                    continue;
                };
                let keep_local = file.local_path.as_ref().is_some_and(|path| path.exists());
                if !keep_local {
                    file.local_path = existing_file
                        .local_path
                        .clone()
                        .filter(|path| path.exists());
                }
                if file.source_path.is_none() {
                    file.source_path = existing_file.source_path.clone();
                }
            }
        }

        item
    }

    pub(super) fn replace_remote_history_snapshot(
        &mut self,
        peer_device_id: &str,
        items: Vec<ClipboardItem>,
    ) {
        let removed_ids = self
            .remote_history
            .replace_peer_items(peer_device_id, items);
        self.cleanup_remote_item_dirs(removed_ids);
    }

    pub(super) fn clear_all_remote_history(&mut self) {
        let removed_ids = self.remote_history.clear();
        self.cleanup_remote_item_dirs(removed_ids);
    }

    pub(super) fn clear_remote_history_for_peer(&mut self, peer_device_id: &str) -> bool {
        self.clear_remote_history_for_peers(std::iter::once(peer_device_id))
    }

    pub(super) fn clear_remote_history_for_peers<'a>(
        &mut self,
        peer_device_ids: impl IntoIterator<Item = &'a str>,
    ) -> bool {
        let mut removed_ids = Vec::new();
        for peer_device_id in peer_device_ids {
            removed_ids.extend(
                self.remote_history
                    .replace_peer_items(peer_device_id, Vec::new()),
            );
        }
        if removed_ids.is_empty() {
            return false;
        }

        self.cleanup_remote_item_dirs(removed_ids);
        true
    }

    pub(super) fn upsert_remote_history_item(&mut self, item: ClipboardItem) {
        let removed_ids = self.remote_history.upsert_live_item(item);
        self.cleanup_remote_item_dirs(removed_ids);
    }

    pub(super) fn upsert_local_history_entry(&mut self, entry: HistoryEntry) {
        Self::upsert_history_entry(&mut self.local_history, entry, self.config.history_limit);
    }

    fn upsert_history_entry(entries: &mut Vec<HistoryEntry>, entry: HistoryEntry, limit: usize) {
        entries.retain(|existing| existing.id != entry.id);
        let insert_index = entries
            .iter()
            .position(|existing| existing.created_at < entry.created_at)
            .unwrap_or(entries.len());
        entries.insert(insert_index, entry);

        let mut unlocked_kept = 0usize;
        entries.retain(|item| {
            if item.is_pinned {
                return true;
            }
            if unlocked_kept < limit.max(1) {
                unlocked_kept += 1;
                true
            } else {
                false
            }
        });
    }

    pub(super) fn cleanup_remote_item_dirs(&mut self, ids: impl IntoIterator<Item = Uuid>) {
        for id in ids {
            let path = self.services.paths.remote_item_dir(id);
            match std::fs::remove_dir_all(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => {
                    self.set_status(format!("清理远程文件缓存失败: {error}"));
                    break;
                }
            }
        }
    }
}
