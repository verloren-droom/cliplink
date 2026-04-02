use std::{
    collections::{HashMap, HashSet},
    time::Instant,
};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use super::remote::RemoteHistorySession;
use crate::core::{
    at_rest::LocalDataCipher,
    clipboard::ClipboardBackend,
    config::{AppConfig, ConfigStore},
    model::{ClipboardKind, DiscoveredPeer},
    paths::AppPaths,
    runtime::RuntimeBridge,
    storage::{HistoryStore, HistoryStoreProfile},
};

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct TickOutcome {
    pub(crate) history_changed: bool,
    pub(crate) devices_changed: bool,
    pub(crate) status_changed: bool,
    pub(crate) transfer_changed: bool,
    pub(crate) paste_requested: bool,
}

/// Runtime activation policy injected by the platform shell.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ControllerRuntimePolicy {
    /// Keeps discovery/network runtime alive even before any device has been trusted.
    pub(crate) keep_discovery_running_without_trusted_peers: bool,
}

impl TickOutcome {
    pub(super) fn merge(&mut self, other: Self) {
        self.history_changed |= other.history_changed;
        self.devices_changed |= other.devices_changed;
        self.status_changed |= other.status_changed;
        self.transfer_changed |= other.transfer_changed;
        self.paste_requested |= other.paste_requested;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HistoryActivation {
    Noop,
    ClipboardReady,
    PendingTransfer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DeviceStatusKind {
    ConnectedTrusted,
    TrustedStandby,
    Offline,
    #[default]
    Untrusted,
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
#[derive(Debug, Clone)]
pub(crate) struct TransferProgressSnapshot {
    pub(crate) item_id: Uuid,
    pub(crate) label: String,
    pub(crate) detail: String,
    pub(crate) fraction: f64,
    pub(crate) source_device_name: String,
    pub(crate) summary: String,
    pub(crate) bytes_done: u64,
    pub(crate) bytes_total: u64,
}

pub(crate) struct AppController {
    pub(super) services: ControllerServices,
    pub(super) config: AppConfig,
    pub(super) local_history: Vec<HistoryEntry>,
    pub(super) remote_history: RemoteHistorySession,
    pub(super) discovered: Vec<DiscoveredPeer>,
    pub(super) device_last_seen: HashMap<String, OffsetDateTime>,
    pub(super) remote_share_enabled: HashMap<String, bool>,
    pub(super) status: Option<String>,
    pub(super) last_clipboard_poll: Instant,
    pub(super) preferences_visible: bool,
    pub(super) pending_trust_requests: Vec<PendingTrustRequest>,
    pub(super) requested_history_sync_peers: HashSet<String>,
    pub(super) announced_share_state_peers: HashSet<String>,
    pub(super) pending_remote_activation: Option<Uuid>,
    pub(super) active_transfer: Option<ActiveTransferState>,
}

pub(super) struct ControllerServices {
    pub(super) paths: AppPaths,
    pub(super) local_data_cipher: LocalDataCipher,
    pub(super) config_store: ConfigStore,
    pub(super) history_store: HistoryStore,
    pub(super) history_store_profile: HistoryStoreProfile,
    pub(super) clipboard: Box<dyn ClipboardBackend>,
    pub(super) runtime: Option<RuntimeBridge>,
    pub(super) runtime_policy: ControllerRuntimePolicy,
}

#[derive(Debug, Clone)]
pub(super) struct ActiveTransferState {
    pub(super) item_id: Uuid,
    pub(super) source_device_name: String,
    pub(super) summary: String,
    pub(super) bytes_done: u64,
    pub(super) bytes_total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct HistoryRow {
    pub(crate) id: Uuid,
    pub(crate) kind: String,
    pub(crate) source_badge: String,
    #[serde(rename = "source_badge_tooltip")]
    pub(crate) source_tooltip: String,
    #[serde(rename = "title")]
    pub(crate) summary_text: String,
    #[serde(rename = "content_tooltip")]
    pub(crate) detail_tooltip: String,
    pub(crate) is_remote: bool,
    #[serde(default)]
    pub(crate) is_pinned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HistoryScopeOption {
    pub(crate) key: String,
    pub(crate) label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) enum HistoryScope {
    All,
    Local,
    Device(String),
}

impl HistoryScope {
    pub(crate) const RAW_ALL: &str = "all";
    pub(crate) const RAW_LOCAL: &str = "local";
    const RAW_DEVICE_PREFIX: &str = "device:";

    pub(crate) fn key(&self) -> String {
        match self {
            Self::All => Self::RAW_ALL.to_string(),
            Self::Local => Self::RAW_LOCAL.to_string(),
            Self::Device(device_id) => format!("{}{}", Self::RAW_DEVICE_PREFIX, device_id),
        }
    }

    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub(crate) fn from_raw(value: &str) -> Self {
        let trimmed = value.trim();
        if trimmed.eq_ignore_ascii_case(Self::RAW_LOCAL) {
            return Self::Local;
        }
        if trimmed.eq_ignore_ascii_case(Self::RAW_ALL) {
            return Self::All;
        }

        if let Some(device_id) = trimmed.strip_prefix(Self::RAW_DEVICE_PREFIX) {
            let device_id = device_id.trim();
            if !device_id.is_empty() {
                return Self::Device(device_id.to_string());
            }
        }

        Self::All
    }
}

#[derive(Debug, Clone)]
pub(super) struct HistoryEntry {
    pub(super) id: Uuid,
    pub(super) kind: ClipboardKind,
    pub(super) created_at: OffsetDateTime,
    pub(super) source_device_id: Option<String>,
    pub(super) source_badge: String,
    pub(super) source_tooltip: String,
    pub(super) summary_text: String,
    pub(super) detail_tooltip: String,
    pub(super) search_blob: String,
    pub(super) is_remote: bool,
    pub(super) is_pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SettingsSnapshot {
    pub(crate) device_name: String,
    pub(crate) history_limit: usize,
    pub(crate) hotkey: String,
    #[serde(default)]
    pub(crate) launch_at_login: bool,
    pub(crate) share_local_history: bool,
    pub(crate) prefer_remote_latest_on_paste: bool,
    pub(crate) discovery_enabled: bool,
    pub(crate) devices: Vec<SettingsDeviceEntry>,
    pub(crate) status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SettingsUpdate {
    pub(crate) device_name: String,
    pub(crate) history_limit: usize,
    pub(crate) hotkey: String,
    #[serde(default)]
    pub(crate) launch_at_login: bool,
    pub(crate) share_local_history: bool,
    pub(crate) prefer_remote_latest_on_paste: bool,
    pub(crate) discovery_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SettingsDeviceEntry {
    pub(crate) device_id: String,
    pub(crate) device_name: String,
    #[serde(rename = "detail")]
    pub(crate) secondary_text: String,
    #[serde(default)]
    pub(crate) status_kind: DeviceStatusKind,
    pub(crate) is_online: bool,
    pub(crate) is_trusted: bool,
    #[serde(default)]
    pub(crate) status_tooltip: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct PendingTrustRequest {
    pub(crate) request_id: Uuid,
    pub(crate) device_id: String,
    pub(crate) device_name: String,
    pub(crate) secondary_text: String,
    pub(crate) fingerprint: String,
}
