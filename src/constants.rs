use std::time::Duration;

/// Application-wide identity and packaging constants shared by all layers.
pub(crate) mod app {
    /// Canonical application display name used across platform shells and packaging.
    pub(crate) const APP_NAME: &str = "ClipLink";

    /// Reverse-DNS application identifier used for platform package metadata.
    #[cfg_attr(target_os = "android", allow(dead_code))]
    pub(crate) const APP_BUNDLE_ID: &str = "com.benfach.cliplink";

    /// Local service hostname used in the self-signed transport certificate.
    pub(crate) const LOCAL_HOSTNAME: &str = "cliplink.local";

    /// Local filesystem folder name for app-owned storage.
    pub(crate) const STORAGE_DIR_NAME: &str = "cliplink";

    /// ProjectDirs qualifier and organization name for app storage roots.
    #[cfg_attr(target_os = "android", allow(dead_code))]
    pub(crate) const ORGANIZATION_QUALIFIER: &str = "com";
    #[cfg_attr(target_os = "android", allow(dead_code))]
    pub(crate) const ORGANIZATION_NAME: &str = "benfach";

    /// Product directory name stored under platform-specific application data roots.
    #[cfg_attr(target_os = "android", allow(dead_code))]
    pub(crate) const PRODUCT_DIR_NAME: &str = "ClipLink";
}

/// Shared filesystem file and directory names used by persistence and startup flows.
pub(crate) mod storage {
    /// Sealed binary configuration file stored under the app root.
    pub(crate) const CONFIG_FILE_NAME: &str = "config.sealed";

    /// SQLite history database file name stored under the app root.
    pub(crate) const HISTORY_DB_FILE_NAME: &str = "history.sqlite3";

    /// Encrypted local transport certificate file name.
    pub(crate) const DEVICE_CERT_FILE_NAME: &str = "device.cert.der";

    /// Encrypted local transport private key file name.
    pub(crate) const DEVICE_KEY_FILE_NAME: &str = "device.key.der";

    /// Per-item inbox directory used to stage fetched remote files.
    pub(crate) const INBOX_DIR_NAME: &str = "incoming";

    /// Single-instance UI lock file name stored under the app root.
    pub(crate) const UI_LOCK_FILE_NAME: &str = "ui.lock";
}

/// LAN discovery protocol constants shared by the runtime and platform shells.
pub(crate) mod discovery {
    /// Fixed binary prefix that marks a UDP datagram as a ClipLink discovery frame.
    pub(crate) const DISCOVERY_PACKET_MAGIC: &[u8; 4] = b"CLKD";

    /// Version number for the current LAN discovery wire format.
    pub(crate) const DISCOVERY_PROTOCOL_VERSION: u8 = 2;

    /// IPv4 multicast group used for ClipLink peer discovery on the local network.
    pub(crate) const DISCOVERY_MULTICAST_GROUP: [u8; 4] = [239, 255, 61, 53];

    /// UDP port shared by all ClipLink peers for discovery probes and announcements.
    pub(crate) const DISCOVERY_PORT: u16 = 27_842;

    /// Maximum accepted size of a discovery datagram.
    pub(crate) const MAX_DISCOVERY_PACKET_BYTES: usize = 4 * 1024;
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
    #[cfg_attr(target_os = "android", allow(dead_code))]
    pub(crate) const LOCAL_DATA_KEY_ACCOUNT: &str = "local-data-master-key";

    /// Fallback file name used on platforms without native secure key storage integration yet.
    #[cfg(all(not(target_os = "macos"), not(target_os = "android")))]
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

    /// Maximum history entries synchronized in a single trusted-peer snapshot request.
    pub(crate) const MAX_HISTORY_SNAPSHOT_ITEMS: usize = 120;

    /// Maximum number of compact remote history entries kept in memory across all peers.
    pub(crate) const MAX_REMOTE_SESSION_ITEMS_TOTAL: usize = 192;

    /// Maximum number of compact remote history entries kept in memory per peer.
    pub(crate) const MAX_REMOTE_SESSION_ITEMS_PER_PEER: usize = 48;

    /// Maximum number of tooltip lines retained for an in-memory remote history item.
    pub(crate) const MAX_REMOTE_HISTORY_TOOLTIP_LINES: usize = 8;

    /// Maximum number of tooltip characters retained for an in-memory remote history item.
    pub(crate) const MAX_REMOTE_HISTORY_TOOLTIP_CHARS: usize = 384;

    /// Maximum number of full remote clipboard items kept in the hot content cache.
    pub(crate) const MAX_REMOTE_CONTENT_CACHE_ITEMS: usize = 2;

    /// Maximum estimated memory budget for the hot remote content cache.
    pub(crate) const MAX_REMOTE_CONTENT_CACHE_BYTES: usize = 256 * 1024;
}

/// Shared timing values used by the runtime, discovery, clipboard, and macOS shell.
pub(crate) mod timing {
    use super::Duration;

    /// Default clipboard polling interval for generic backends.
    pub(crate) const CLIPBOARD_POLL_INTERVAL: Duration = Duration::from_millis(250);

    /// Faster clipboard polling interval for the macOS pasteboard backend.
    #[cfg_attr(target_os = "android", allow(dead_code))]
    pub(crate) const MACOS_CLIPBOARD_POLL_INTERVAL: Duration = Duration::from_millis(120);

    /// Short focus-settle delay before replaying paste to the previously focused app.
    #[cfg_attr(target_os = "android", allow(dead_code))]
    pub(crate) const IMMEDIATE_PASTE_DELAY: Duration = Duration::from_millis(16);

    /// AppKit timer cadence for UI-driven controller ticks.
    #[cfg_attr(target_os = "android", allow(dead_code))]
    pub(crate) const MACOS_UI_TICK_INTERVAL_SECONDS: f64 = 0.12;

    /// QUIC keepalive cadence used to preserve warm peer connections between clipboard events.
    pub(crate) const QUIC_KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(30);

    /// Periodic LAN presence heartbeat cadence.
    pub(crate) const DISCOVERY_ANNOUNCE_INTERVAL: Duration = Duration::from_secs(2);

    /// Retry delay used after discovery socket setup or send/receive failures.
    pub(crate) const DISCOVERY_RETRY_INTERVAL: Duration = Duration::from_secs(3);

    /// Grace period before a previously discovered peer is considered offline.
    pub(crate) const DISCOVERY_OFFLINE_GRACE: Duration = Duration::from_secs(8);
}

/// Shared transfer sizing values used by the protocol and runtime streaming logic.
pub(crate) mod transfer {
    /// Maximum allowed size for a single length-prefixed protocol frame.
    pub(crate) const MAX_PROTOCOL_FRAME_BYTES: usize = 4 * 1024 * 1024;

    /// Buffered file I/O chunk size used by QUIC file streaming.
    pub(crate) const FILE_STREAM_CHUNK_BYTES: usize = 64 * 1024;

    /// Tight upper bound for concurrently open bidirectional QUIC streams per peer.
    pub(crate) const QUIC_MAX_CONCURRENT_BIDI_STREAMS: u8 = 4;

    /// Per-stream QUIC receive window tuned for clipboard payloads and chunked file transfer.
    pub(crate) const QUIC_STREAM_RECEIVE_WINDOW_BYTES: u32 = 256 * 1024;

    /// Connection-level QUIC receive window kept intentionally small to limit idle memory.
    pub(crate) const QUIC_CONNECTION_RECEIVE_WINDOW_BYTES: u32 = 1024 * 1024;

    /// QUIC send window matching the connection receive budget to avoid excessive buffering.
    pub(crate) const QUIC_SEND_WINDOW_BYTES: u64 = 1024 * 1024;

    /// Minimum byte delta before reporting another file transfer progress update.
    pub(crate) const PROGRESS_EMIT_GRANULARITY_BYTES: u64 = 512 * 1024;
}
