use std::time::Duration;

/// Application-wide identity and packaging constants shared by all layers.
pub(crate) mod app {
    /// Canonical application display name used across platform shells and packaging.
    pub(crate) const APP_NAME: &str = "ClipLink";

    /// Reverse-DNS application identifier used for platform package metadata.
    pub(crate) const APP_BUNDLE_ID: &str = "com.benfach.cliplink";

    /// Local service hostname used in the self-signed transport certificate.
    pub(crate) const LOCAL_HOSTNAME: &str = "cliplink.local";

    /// mDNS service type advertised on the local network.
    pub(crate) const MDNS_SERVICE_TYPE: &str = "_cliplink._udp";

    /// Local filesystem folder name for app-owned storage.
    pub(crate) const STORAGE_DIR_NAME: &str = "cliplink";

    /// ProjectDirs qualifier and organization name for app storage roots.
    pub(crate) const ORGANIZATION_QUALIFIER: &str = "com";
    pub(crate) const ORGANIZATION_NAME: &str = "benfach";

    /// Product directory name stored under platform-specific application data roots.
    pub(crate) const PRODUCT_DIR_NAME: &str = "ClipLink";
}

/// Shared local-at-rest encryption constants used by config, history, and identity persistence.
pub(crate) mod crypto {
    /// Fixed magic prefix that marks a file or blob as a sealed local data payload.
    pub(crate) const LOCAL_DATA_MAGIC: &[u8; 8] = b"CLKSEAL1";

    /// Master key length used for local AEAD encryption.
    pub(crate) const LOCAL_DATA_KEY_BYTES: usize = 32;

    /// Purpose binding for the encrypted config payload.
    pub(crate) const CONFIG_PURPOSE: &[u8] = b"cliplink.config.v1";

    /// Purpose binding for the encrypted history payload stored in SQLite.
    pub(crate) const HISTORY_PURPOSE: &[u8] = b"cliplink.history.v1";

    /// Purpose binding for the persisted transport certificate.
    pub(crate) const DEVICE_CERT_PURPOSE: &[u8] = b"cliplink.device-cert.v1";

    /// Purpose binding for the persisted transport private key.
    pub(crate) const DEVICE_KEY_PURPOSE: &[u8] = b"cliplink.device-key.v1";

    /// Account name used to store the local master key in secure platform key storage.
    pub(crate) const LOCAL_DATA_KEY_ACCOUNT: &str = "local-data-master-key";

    /// Fallback file name used on platforms without native secure key storage integration yet.
    #[cfg(not(target_os = "macos"))]
    pub(crate) const LOCAL_DATA_KEY_FILE_NAME: &str = "local-data.key";
}

/// Shared configuration defaults surfaced across controller and platform layers.
pub(crate) mod config {
    /// Default history popup hotkey shown in preferences and used when no custom value exists.
    pub(crate) const DEFAULT_HISTORY_POPUP_HOTKEY: &str = "CmdOrCtrl+Shift+V";

    /// Default QUIC listen port for local peer communication.
    pub(crate) const DEFAULT_LISTEN_PORT: u16 = 27_841;

    /// Default number of clipboard records kept in history.
    pub(crate) const DEFAULT_HISTORY_LIMIT: usize = 120;
}

/// Cross-layer validation and presentation limits that must stay consistent.
pub(crate) mod limits {
    /// Maximum allowed length for the local device name stored in preferences.
    pub(crate) const MAX_DEVICE_NAME_CHARS: usize = 64;

    /// Maximum number of history items accepted by settings validation.
    pub(crate) const MAX_HISTORY_LIMIT: usize = 500;

    /// Maximum number of tooltip lines rendered for a history item's detail preview.
    pub(crate) const MAX_HISTORY_TOOLTIP_LINES: usize = 32;

    /// Maximum number of tooltip characters kept in memory for a history item's detail preview.
    pub(crate) const MAX_HISTORY_TOOLTIP_CHARS: usize = 2_048;
}

/// Shared timing values used by the runtime, discovery, clipboard, and macOS shell.
pub(crate) mod timing {
    use super::Duration;

    /// Timeout for a single mDNS discovery sweep.
    pub(crate) const DISCOVERY_QUERY_TIMEOUT: Duration = Duration::from_millis(150);

    /// Default clipboard polling interval for generic backends.
    pub(crate) const CLIPBOARD_POLL_INTERVAL: Duration = Duration::from_millis(400);

    /// Faster clipboard polling interval for the macOS pasteboard backend.
    pub(crate) const MACOS_CLIPBOARD_POLL_INTERVAL: Duration = Duration::from_millis(250);

    /// Short focus-settle delay before replaying paste to the previously focused app.
    pub(crate) const MACOS_IMMEDIATE_PASTE_DELAY: Duration = Duration::from_millis(16);

    /// AppKit timer cadence for UI-driven controller ticks.
    pub(crate) const MACOS_UI_TICK_INTERVAL_SECONDS: f64 = 0.25;

    /// Background discovery refresh cadence.
    pub(crate) const RUNTIME_DISCOVERY_INTERVAL: Duration = Duration::from_secs(3);
}
