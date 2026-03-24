use std::{
    collections::{BTreeMap, HashSet},
    time::Instant,
};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    constants::{
        config::DEFAULT_HISTORY_POPUP_HOTKEY,
        limits::{
            MAX_DEVICE_NAME_CHARS, MAX_HISTORY_LIMIT, MAX_HISTORY_TOOLTIP_CHARS,
            MAX_HISTORY_TOOLTIP_LINES,
        },
    },
    core::{
        at_rest::LocalDataCipher,
        clipboard::{ClipboardBackend, build_item_from_text},
        config::{AppConfig, ConfigStore},
        error::{AppError, AppResult},
        model::{ClipboardItem, ClipboardPayload, DiscoveredPeer, TrustedPeer},
        paths::AppPaths,
        runtime::{RuntimeBridge, RuntimeCommand, RuntimeEvent},
        storage::HistoryStore,
    },
};

#[derive(Debug, Default, Clone, Copy)]
pub struct TickOutcome {
    pub history_changed: bool,
    pub devices_changed: bool,
    pub status_changed: bool,
}

impl TickOutcome {
    pub const fn preferences_changed(self) -> bool {
        self.devices_changed || self.status_changed
    }

    fn merge(&mut self, other: Self) {
        self.history_changed |= other.history_changed;
        self.devices_changed |= other.devices_changed;
        self.status_changed |= other.status_changed;
    }
}

pub struct AppController {
    services: ControllerServices,
    config: AppConfig,
    history: Vec<HistoryEntry>,
    discovered: Vec<DiscoveredPeer>,
    status: Option<String>,
    last_clipboard_poll: Instant,
    preferences_visible: bool,
}

struct ControllerServices {
    paths: AppPaths,
    local_data_cipher: LocalDataCipher,
    config_store: ConfigStore,
    history_store: HistoryStore,
    clipboard: Box<dyn ClipboardBackend>,
    runtime: Option<RuntimeBridge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryRow {
    pub id: Uuid,
    pub source_badge: String,
    #[serde(rename = "source_badge_tooltip")]
    pub source_tooltip: String,
    #[serde(rename = "title")]
    pub summary_text: String,
    #[serde(rename = "content_tooltip")]
    pub detail_tooltip: String,
    pub is_remote: bool,
    #[serde(default)]
    pub is_pinned: bool,
}

#[derive(Debug, Clone)]
struct HistoryEntry {
    id: Uuid,
    signature: String,
    source_badge: String,
    source_tooltip: String,
    summary_text: String,
    detail_tooltip: String,
    search_blob: String,
    is_remote: bool,
    is_pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsSnapshot {
    pub device_name: String,
    pub history_limit: usize,
    pub hotkey: String,
    #[serde(default)]
    pub launch_at_login: bool,
    pub share_local_history: bool,
    pub prefer_remote_latest_on_paste: bool,
    pub discovery_enabled: bool,
    pub devices: Vec<SettingsDeviceEntry>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsUpdate {
    pub device_name: String,
    pub history_limit: usize,
    pub hotkey: String,
    #[serde(default)]
    pub launch_at_login: bool,
    pub share_local_history: bool,
    pub prefer_remote_latest_on_paste: bool,
    pub discovery_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsDeviceEntry {
    pub device_id: String,
    pub device_name: String,
    #[serde(rename = "detail")]
    pub secondary_text: String,
    pub is_online: bool,
    pub is_trusted: bool,
}

impl AppController {
    pub fn bootstrap(
        paths: AppPaths,
        clipboard: Box<dyn ClipboardBackend>,
        local_data_cipher: LocalDataCipher,
        device_name_hint: Option<&str>,
    ) -> AppResult<Self> {
        let config_store = ConfigStore::new(&paths, local_data_cipher.clone());
        let config = config_store.load_or_init_normalized(device_name_hint)?;
        let poll_interval = clipboard.recommended_poll_interval();

        let history_store = HistoryStore::open(&paths, local_data_cipher.clone())?;
        let history = history_store
            .recent(config.history_limit)?
            .into_iter()
            .map(|item| Self::build_history_entry(&item))
            .collect();
        let now = Instant::now();
        let last_clipboard_poll = now.checked_sub(poll_interval).unwrap_or(now);

        let mut controller = Self {
            services: ControllerServices {
                paths,
                local_data_cipher,
                config_store,
                history_store,
                clipboard,
                runtime: None,
            },
            config,
            history,
            discovered: Vec::new(),
            status: None,
            last_clipboard_poll,
            preferences_visible: false,
        };
        controller.sync_runtime()?;
        Ok(controller)
    }

    pub fn hotkey(&self) -> &str {
        if self.config.hotkey.trim().is_empty() {
            DEFAULT_HISTORY_POPUP_HOTKEY
        } else {
            self.config.hotkey.as_str()
        }
    }

    pub fn settings_snapshot(&self) -> SettingsSnapshot {
        SettingsSnapshot {
            device_name: self.config.device_name.clone(),
            history_limit: self.config.history_limit,
            hotkey: self.hotkey().to_string(),
            launch_at_login: self.config.launch_at_login,
            share_local_history: self.config.share_local_history,
            prefer_remote_latest_on_paste: self.config.prefer_remote_latest_on_paste,
            discovery_enabled: self.config.discovery_enabled,
            devices: self.device_entries(),
            status: self.status.clone(),
        }
    }

    pub fn current_settings_update(&self) -> SettingsUpdate {
        SettingsUpdate {
            device_name: self.config.device_name.clone(),
            history_limit: self.config.history_limit,
            hotkey: self.hotkey().to_string(),
            launch_at_login: self.config.launch_at_login,
            share_local_history: self.config.share_local_history,
            prefer_remote_latest_on_paste: self.config.prefer_remote_latest_on_paste,
            discovery_enabled: self.config.discovery_enabled,
        }
    }

    pub fn normalize_settings_update(&self, mut update: SettingsUpdate) -> SettingsUpdate {
        update.device_name = if update.device_name.trim().is_empty() {
            self.config.device_name.clone()
        } else {
            update.device_name.trim().to_string()
        };
        update.history_limit = update.history_limit.clamp(1, MAX_HISTORY_LIMIT);
        update.hotkey = if update.hotkey.trim().is_empty() {
            DEFAULT_HISTORY_POPUP_HOTKEY.to_string()
        } else {
            update.hotkey.trim().to_string()
        };
        update
    }

    pub fn validate_settings_update(&self, update: &SettingsUpdate) -> AppResult<()> {
        let device_name = update.device_name.trim();
        if device_name.is_empty() {
            return Err(AppError::InvalidConfig(
                "Device name cannot be empty.".to_string(),
            ));
        }
        if device_name.chars().count() > MAX_DEVICE_NAME_CHARS {
            return Err(AppError::InvalidConfig(format!(
                "Device name must be at most {MAX_DEVICE_NAME_CHARS} characters."
            )));
        }
        if device_name.chars().any(char::is_control) {
            return Err(AppError::InvalidConfig(
                "Device name cannot contain control characters.".to_string(),
            ));
        }
        if update.history_limit == 0 || update.history_limit > MAX_HISTORY_LIMIT {
            return Err(AppError::InvalidConfig(format!(
                "History limit must be between 1 and {MAX_HISTORY_LIMIT}."
            )));
        }

        let hotkey = update.hotkey.trim();
        if hotkey.is_empty() {
            return Err(AppError::InvalidConfig(
                "History popup hotkey cannot be empty.".to_string(),
            ));
        }

        Ok(())
    }

    pub fn apply_settings(&mut self, update: SettingsUpdate) -> AppResult<()> {
        let update = self.normalize_settings_update(update);
        self.validate_settings_update(&update)?;
        let mut next = self.config.clone();
        let paste_preference_changed =
            self.config.prefer_remote_latest_on_paste != update.prefer_remote_latest_on_paste;
        next.device_name = update.device_name;
        next.history_limit = update.history_limit;
        next.hotkey = update.hotkey;
        next.launch_at_login = update.launch_at_login;
        next.share_local_history = update.share_local_history;
        next.prefer_remote_latest_on_paste = update.prefer_remote_latest_on_paste;
        next.auto_sync = true;
        next.discovery_enabled = update.discovery_enabled;
        self.services.config_store.save(&next)?;
        self.services
            .history_store
            .enforce_limit(next.history_limit)?;
        self.config = next.clone();
        self.history = self
            .services
            .history_store
            .recent(self.config.history_limit)?
            .into_iter()
            .map(|item| Self::build_history_entry(&item))
            .collect();
        let runtime_sync_error = self.sync_runtime().err();
        let clipboard_sync_error = if paste_preference_changed {
            self.sync_preferred_latest_to_clipboard().err()
        } else {
            None
        };

        let mut status = "设置已保存".to_string();
        if let Some(error) = runtime_sync_error {
            status.push_str(&format!("，但后台网络服务更新失败: {error}"));
        }
        if let Some(error) = clipboard_sync_error {
            status.push_str(&format!("，但更新系统剪切板失败: {error}"));
        }
        self.set_status(status);
        Ok(())
    }

    pub fn set_preferences_visible(&mut self, visible: bool) -> AppResult<()> {
        if self.preferences_visible == visible {
            return Ok(());
        }

        self.preferences_visible = visible;
        self.sync_runtime()
    }

    pub fn launch_at_login_enabled(&self) -> bool {
        self.config.launch_at_login
    }

    pub fn report_status(&mut self, status: impl Into<String>) {
        self.set_status(status);
    }

    pub fn history_rows(&self, query: &str) -> Vec<HistoryRow> {
        let query = query.trim().to_lowercase();
        self.history
            .iter()
            .filter(|item| query.is_empty() || item.search_blob.contains(query.as_str()))
            .map(Self::history_entry_to_row)
            .collect()
    }

    pub fn load_item(&self, id: Uuid) -> AppResult<Option<ClipboardItem>> {
        self.services.history_store.find_by_id(id)
    }

    pub fn recent_menu_entries(&self, limit: usize) -> Vec<(Uuid, String)> {
        self.history
            .iter()
            .take(limit)
            .map(|item| {
                (
                    item.id,
                    format!("[{}] {}", item.source_badge, item.summary_text),
                )
            })
            .collect()
    }

    pub fn clear_history(&mut self) -> AppResult<()> {
        let deleted = self.services.history_store.clear_unpinned()?;
        self.history.retain(|item| item.is_pinned);
        if deleted == 0 {
            self.set_status("没有可清理的未锁定历史");
        } else if self.history.is_empty() {
            self.set_status("已清空剪切板历史");
        } else {
            self.set_status("已清空未锁定历史，锁定项已保留");
        }
        Ok(())
    }

    pub fn delete_history_item(&mut self, id: Uuid) -> AppResult<bool> {
        if !self.services.history_store.delete_by_id(id)? {
            return Ok(false);
        }

        let before = self.history.len();
        self.history.retain(|item| item.id != id);
        if self.history.len() != before {
            self.set_status("已删除剪切板历史");
        }
        Ok(true)
    }

    pub fn toggle_history_item_pin(&mut self, id: Uuid) -> AppResult<Option<bool>> {
        let Some(mut item) = self.load_item(id)? else {
            return Ok(None);
        };

        item.is_pinned = !item.is_pinned;
        self.services.history_store.update_item(&item)?;
        if let Some(entry) = self.history.iter_mut().find(|entry| entry.id == id) {
            entry.is_pinned = item.is_pinned;
        } else {
            self.prepend_history_entry(Self::build_history_entry(&item));
        }
        self.set_status(if item.is_pinned {
            "已锁定该历史条目"
        } else {
            "已取消锁定该历史条目"
        });
        Ok(Some(item.is_pinned))
    }

    pub fn trust_device(&mut self, device_id: &str) -> AppResult<bool> {
        let Some(peer) = self
            .discovered
            .iter()
            .find(|peer| peer.device_id == device_id)
            .cloned()
        else {
            return Ok(false);
        };

        let next_peer = TrustedPeer {
            device_id: peer.device_id.clone(),
            device_name: peer.device_name.clone(),
            fingerprint: peer.fingerprint.clone(),
        };

        let mut updated = false;
        if let Some(existing) = self
            .config
            .trusted_peers
            .iter_mut()
            .find(|existing| existing.device_id == next_peer.device_id)
        {
            if *existing != next_peer {
                *existing = next_peer;
                updated = true;
            }
        } else {
            self.config.trusted_peers.push(next_peer);
            updated = true;
        }

        if !updated {
            return Ok(false);
        }

        self.config
            .trusted_peers
            .sort_by(|left, right| left.device_name.cmp(&right.device_name));
        self.services.config_store.save(&self.config)?;
        self.sync_runtime()?;
        self.set_status(format!("已信任设备 {}", peer.device_name));
        Ok(true)
    }

    pub fn revoke_device_trust(&mut self, device_id: &str) -> AppResult<bool> {
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
            return Ok(false);
        }

        self.services.config_store.save(&self.config)?;
        self.sync_runtime()?;
        self.set_status(format!(
            "已移除信任设备 {}",
            removed_name.unwrap_or_else(|| "未知设备".to_string())
        ));
        Ok(true)
    }

    pub fn copy_item(&mut self, id: Uuid) -> AppResult<bool> {
        let Some(mut item) = self.load_item(id)? else {
            return Ok(false);
        };

        item.created_at = OffsetDateTime::now_utc();
        self.services.history_store.update_item(&item)?;
        self.prepend_history_entry(Self::build_history_entry(&item));
        self.services.clipboard.write_item(&item)?;
        self.set_status("已将历史记录写回系统剪切板");
        if self.config.share_local_history {
            if let Err(error) = self.broadcast_item(item.clone()) {
                self.set_status(format!("已将历史记录写回系统剪切板，但共享失败: {error}"));
            }
        }
        Ok(true)
    }

    /// Ingests an externally observed local clipboard payload without waiting for backend polling.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub fn submit_local_clipboard_text(&mut self, text: &str) -> TickOutcome {
        let text = text.trim();
        if text.is_empty() {
            return TickOutcome::default();
        }

        let item = build_item_from_text(
            text,
            Some(self.config.device_id.clone()),
            Some(self.config.device_name.clone()),
            false,
        );
        self.handle_local_clipboard_item(item)
    }

    pub fn tick(&mut self) -> TickOutcome {
        let mut outcome = TickOutcome::default();

        while let Some(event) = self
            .services
            .runtime
            .as_ref()
            .and_then(RuntimeBridge::try_recv)
        {
            match event {
                RuntimeEvent::PeerList(peers) => {
                    self.discovered = peers;
                    outcome.devices_changed = true;
                }
                RuntimeEvent::InboundClipboard(item) => {
                    let inbound_persist_error = self
                        .services
                        .history_store
                        .insert(&item, self.config.history_limit)
                        .err();
                    self.prepend_history_entry(Self::build_history_entry(&item));
                    outcome.history_changed = true;
                    let inbound_source = item
                        .source_device_name
                        .clone()
                        .unwrap_or_else(|| "远端设备".to_string());

                    match self.sync_preferred_latest_to_clipboard() {
                        Ok(Some(preferred_item))
                            if preferred_item.id != item.id
                                && !self.config.prefer_remote_latest_on_paste =>
                        {
                            let mut status = format!(
                                "已接收来自 {inbound_source} 的内容，粘贴仍优先本机最新记录"
                            );
                            if let Some(error) = inbound_persist_error.as_ref() {
                                status.push_str(&format!("，但保存历史失败: {error}"));
                            }
                            self.set_status(status);
                            outcome.status_changed = true;
                        }
                        Ok(_) => {
                            let mut status = format!("已接收来自 {inbound_source} 的内容");
                            if let Some(error) = inbound_persist_error.as_ref() {
                                status.push_str(&format!("，但保存历史失败: {error}"));
                            }
                            self.set_status(status);
                            outcome.status_changed = true;
                        }
                        Err(error) => {
                            let mut status = format!("更新系统剪切板失败: {error}");
                            if let Some(store_error) = inbound_persist_error.as_ref() {
                                status.push_str(&format!("；保存历史失败: {store_error}"));
                            }
                            self.set_status(status);
                            outcome.status_changed = true;
                        }
                    }
                }
                RuntimeEvent::Status(status) => {
                    self.set_status(status);
                    outcome.status_changed = true;
                }
            }
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

    fn prepend_history_entry(&mut self, entry: HistoryEntry) {
        self.history
            .retain(|existing| existing.signature != entry.signature);
        self.history.insert(0, entry);
        let mut unlocked_kept = 0usize;
        self.history.retain(|item| {
            if item.is_pinned {
                return true;
            }
            if unlocked_kept < self.config.history_limit.max(1) {
                unlocked_kept += 1;
                true
            } else {
                false
            }
        });
    }

    fn handle_local_clipboard_item(&mut self, item: ClipboardItem) -> TickOutcome {
        let mut outcome = TickOutcome::default();
        match self
            .services
            .history_store
            .insert(&item, self.config.history_limit)
        {
            Ok(true) => {
                self.prepend_history_entry(Self::build_history_entry(&item));
                outcome.history_changed = true;
                if self.config.share_local_history {
                    if let Err(error) = self.broadcast_item(item) {
                        self.set_status(format!("本机内容已保存，但共享失败: {error}"));
                        outcome.status_changed = true;
                    }
                }
            }
            Ok(false) => {}
            Err(error) => {
                self.set_status(format!("保存本机剪切板历史失败: {error}"));
                outcome.status_changed = true;
            }
        }
        outcome
    }

    fn runtime_should_run(&self) -> bool {
        !self.config.trusted_peers.is_empty()
            || (self.preferences_visible && self.config.discovery_enabled)
    }

    fn ensure_runtime_started(&mut self) -> AppResult<()> {
        if self.services.runtime.is_none() {
            self.services.runtime = Some(RuntimeBridge::start(
                self.services.paths.clone(),
                self.config.clone(),
                self.services.local_data_cipher.clone(),
            )?);
        }
        Ok(())
    }

    fn stop_runtime(&mut self) {
        if self.services.runtime.take().is_some() {
            self.discovered.clear();
        }
    }

    fn sync_runtime(&mut self) -> AppResult<()> {
        if !self.runtime_should_run() {
            self.stop_runtime();
            return Ok(());
        }

        self.ensure_runtime_started()?;
        if let Some(runtime) = self.services.runtime.as_ref() {
            runtime.send(RuntimeCommand::ReplaceConfig(self.config.clone()));
        }
        Ok(())
    }

    fn broadcast_item(&mut self, item: ClipboardItem) -> AppResult<()> {
        if self.services.runtime.is_none() && !self.config.trusted_peers.is_empty() {
            self.ensure_runtime_started()?;
        }

        if let Some(runtime) = self.services.runtime.as_ref() {
            runtime.send(RuntimeCommand::Broadcast(item));
        }
        Ok(())
    }

    fn preferred_latest_item(&self) -> AppResult<Option<ClipboardItem>> {
        let Some(latest) = self.history.first() else {
            return Ok(None);
        };
        if latest.is_remote && !self.config.prefer_remote_latest_on_paste {
            if let Some(item) = self
                .history
                .iter()
                .find(|item| !item.is_remote)
                .map(|item| item.id)
            {
                return self.load_item(item);
            }
            self.load_item(latest.id)
        } else {
            self.load_item(latest.id)
        }
    }

    fn sync_preferred_latest_to_clipboard(&mut self) -> AppResult<Option<ClipboardItem>> {
        let Some(item) = self.preferred_latest_item()? else {
            return Ok(None);
        };

        self.services.clipboard.write_item(&item)?;
        Ok(Some(item))
    }

    fn set_status(&mut self, status: impl Into<String>) {
        self.status = Some(status.into());
    }

    fn history_entry_to_row(entry: &HistoryEntry) -> HistoryRow {
        HistoryRow {
            id: entry.id,
            source_badge: entry.source_badge.clone(),
            source_tooltip: entry.source_tooltip.clone(),
            summary_text: entry.summary_text.clone(),
            detail_tooltip: entry.detail_tooltip.clone(),
            is_remote: entry.is_remote,
            is_pinned: entry.is_pinned,
        }
    }

    fn build_history_entry(item: &ClipboardItem) -> HistoryEntry {
        let search_blob = item.compact_search_blob(512).to_lowercase();
        HistoryEntry {
            id: item.id,
            signature: item.signature.clone(),
            source_badge: Self::item_source_badge_text(item),
            source_tooltip: Self::item_source_tooltip(item),
            summary_text: Self::item_summary_text(item),
            detail_tooltip: Self::item_detail_tooltip(item),
            search_blob,
            is_remote: item.is_remote,
            is_pinned: item.is_pinned,
        }
    }

    fn item_detail_tooltip(item: &ClipboardItem) -> String {
        match &item.payload {
            ClipboardPayload::Text(text) => {
                if text.is_empty() {
                    "空文本内容".to_string()
                } else {
                    Self::limit_tooltip_text(text)
                }
            }
            ClipboardPayload::Files(files) => {
                Self::limit_tooltip_text(&Self::files_content_tooltip(files))
            }
        }
    }

    fn item_source_badge_text(item: &ClipboardItem) -> String {
        if item.is_remote {
            item.source_device_name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(Self::truncate_badge_label)
                .unwrap_or_else(|| "远端".to_string())
        } else {
            "本机".to_string()
        }
    }

    fn item_source_tooltip(item: &ClipboardItem) -> String {
        if item.is_remote {
            item.source_device_name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .unwrap_or("远端")
                .to_string()
        } else {
            item.source_device_name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .unwrap_or("本机")
                .to_string()
        }
    }

    fn item_summary_text(item: &ClipboardItem) -> String {
        match &item.payload {
            ClipboardPayload::Text(text) => text
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(|line| Self::truncate_inline(line, 240))
                .unwrap_or_else(|| "空文本内容".to_string()),
            ClipboardPayload::Files(files) => {
                if files.is_empty() {
                    return "无文件内容".to_string();
                }

                if files.len() == 1 {
                    let summary = format!("文件 · {}", files[0].relative_path);
                    return Self::truncate_inline(&summary, 240);
                }

                let lead = files
                    .first()
                    .map(|file| file.relative_path.as_str())
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or("文件");
                let prefix = format!("{} 个文件", files.len());
                let summary = format!("{prefix} · {lead}");
                if summary.trim().is_empty() {
                    "文件内容".to_string()
                } else {
                    Self::truncate_inline(&summary, 240)
                }
            }
        }
    }

    fn files_content_tooltip(files: &[crate::core::model::FileDescriptor]) -> String {
        if files.is_empty() {
            return "无文件内容".to_string();
        }

        #[derive(Default)]
        struct TopLevelEntry {
            name: String,
            display_path: String,
            is_directory: bool,
            file_count: usize,
            total_bytes: u64,
        }

        let mut entries = BTreeMap::<String, TopLevelEntry>::new();
        for file in files {
            let relative_path = file.relative_path.trim().trim_matches('/');
            if relative_path.is_empty() {
                continue;
            }

            let mut parts = relative_path.split('/');
            let top_level = parts.next().unwrap_or(relative_path);
            let has_nested = parts.next().is_some();
            let entry = entries
                .entry(top_level.to_string())
                .or_insert_with(|| TopLevelEntry {
                    name: top_level.to_string(),
                    display_path: relative_path.to_string(),
                    is_directory: has_nested,
                    file_count: 0,
                    total_bytes: 0,
                });
            entry.is_directory |= has_nested;
            entry.file_count += 1;
            entry.total_bytes = entry.total_bytes.saturating_add(file.size_bytes);
            if entry.display_path.len() > relative_path.len() {
                entry.display_path = relative_path.to_string();
            }
        }

        if entries.is_empty() {
            return "无文件内容".to_string();
        }

        if entries.len() == 1 {
            let entry = entries.into_values().next().unwrap_or_default();
            if entry.is_directory {
                return format!(
                    "文件夹\n名称: {}\n包含文件: {}\n总大小: {}",
                    entry.name,
                    entry.file_count,
                    Self::format_bytes(entry.total_bytes)
                );
            }

            return format!(
                "文件\n名称: {}\n路径: {}\n大小: {}",
                entry.name,
                entry.display_path,
                Self::format_bytes(entry.total_bytes)
            );
        }

        let mut lines = Vec::with_capacity(entries.len() + 1);
        lines.push(format!("共 {} 项", entries.len()));
        for entry in entries.into_values() {
            if entry.is_directory {
                lines.push(format!(
                    "文件夹 · {} · {} 个文件 · {}",
                    entry.name,
                    entry.file_count,
                    Self::format_bytes(entry.total_bytes)
                ));
            } else {
                lines.push(format!(
                    "文件 · {} · {}",
                    entry.display_path,
                    Self::format_bytes(entry.total_bytes)
                ));
            }
        }
        lines.join("\n")
    }

    fn truncate_badge_label(text: &str) -> String {
        Self::truncate_inline(text, 8)
    }

    fn truncate_inline(text: &str, max_chars: usize) -> String {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return String::new();
        }

        let mut value = trimmed.chars().take(max_chars).collect::<String>();
        if trimmed.chars().count() > max_chars {
            value.push('…');
        }
        value
    }

    fn limit_tooltip_text(text: &str) -> String {
        if text.is_empty() {
            return String::new();
        }

        let mut clipped = String::with_capacity(text.len().min(MAX_HISTORY_TOOLTIP_CHARS));
        let mut clipped_chars = 0usize;
        let mut seen_lines = 0usize;
        let mut omitted_lines = 0usize;
        let mut was_truncated_by_chars = false;

        for line in text.lines() {
            if seen_lines >= MAX_HISTORY_TOOLTIP_LINES {
                omitted_lines += 1;
                continue;
            }

            if !clipped.is_empty() {
                if clipped_chars >= MAX_HISTORY_TOOLTIP_CHARS {
                    was_truncated_by_chars = true;
                    omitted_lines += 1;
                    continue;
                }
                clipped.push('\n');
                clipped_chars += 1;
            }

            let remaining_chars = MAX_HISTORY_TOOLTIP_CHARS.saturating_sub(clipped_chars);
            if remaining_chars == 0 {
                was_truncated_by_chars = true;
                omitted_lines += 1;
                continue;
            }

            let line_len = line.chars().count();
            if line_len <= remaining_chars {
                clipped.push_str(line);
                clipped_chars += line_len;
            } else {
                for (index, ch) in line.chars().enumerate() {
                    if index + 1 >= remaining_chars {
                        break;
                    }
                    clipped.push(ch);
                    clipped_chars += 1;
                }
                clipped.push('…');
                clipped_chars += 1;
                was_truncated_by_chars = true;
            }

            seen_lines += 1;
        }

        if omitted_lines > 0 {
            if !clipped.is_empty() {
                clipped.push('\n');
            }
            clipped.push_str(&format!("……已省略 {omitted_lines} 行"));
        } else if was_truncated_by_chars {
            if !clipped.is_empty() {
                clipped.push('\n');
            }
            clipped.push_str("……内容已截断");
        }

        clipped
    }

    fn format_bytes(bytes: u64) -> String {
        const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];

        let mut value = bytes as f64;
        let mut unit_index = 0usize;
        while value >= 1024.0 && unit_index + 1 < UNITS.len() {
            value /= 1024.0;
            unit_index += 1;
        }

        if unit_index == 0 {
            format!("{bytes} {}", UNITS[unit_index])
        } else if value >= 100.0 {
            format!("{value:.0} {}", UNITS[unit_index])
        } else if value >= 10.0 {
            format!("{value:.1} {}", UNITS[unit_index])
        } else {
            format!("{value:.2} {}", UNITS[unit_index])
        }
    }

    fn device_entries(&self) -> Vec<SettingsDeviceEntry> {
        let trusted_ids = self
            .config
            .trusted_peers
            .iter()
            .map(|peer| peer.device_id.clone())
            .collect::<HashSet<_>>();
        let mut seen = HashSet::new();
        let mut entries = self
            .discovered
            .iter()
            .map(|peer| {
                seen.insert(peer.device_id.clone());
                let is_trusted = trusted_ids.contains(&peer.device_id);
                SettingsDeviceEntry {
                    device_id: peer.device_id.clone(),
                    device_name: peer.device_name.clone(),
                    secondary_text: format!(
                        "{}  {}:{}",
                        if is_trusted {
                            "已信任 · 在线"
                        } else {
                            "未信任 · 在线"
                        },
                        peer.address,
                        peer.port
                    ),
                    is_online: true,
                    is_trusted,
                }
            })
            .collect::<Vec<_>>();

        for peer in &self.config.trusted_peers {
            if seen.contains(&peer.device_id) {
                continue;
            }
            entries.push(SettingsDeviceEntry {
                device_id: peer.device_id.clone(),
                device_name: peer.device_name.clone(),
                secondary_text: "已信任 · 未在线".to_string(),
                is_online: false,
                is_trusted: true,
            });
        }

        entries.sort_by(|left, right| {
            right
                .is_trusted
                .cmp(&left.is_trusted)
                .then_with(|| right.is_online.cmp(&left.is_online))
                .then_with(|| left.device_name.cmp(&right.device_name))
        });
        entries
    }
}
