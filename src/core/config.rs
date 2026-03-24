use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    constants::{
        app::APP_NAME,
        config::{DEFAULT_HISTORY_LIMIT, DEFAULT_HISTORY_POPUP_HOTKEY, DEFAULT_LISTEN_PORT},
        crypto::CONFIG_PURPOSE,
    },
    core::{
        at_rest::{LocalDataCipher, load_or_legacy_bytes, save_sealed_bytes},
        error::AppResult,
        model::TrustedPeer,
        paths::AppPaths,
    },
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AppConfig {
    pub device_id: String,
    pub device_name: String,
    pub listen_port: u16,
    pub history_limit: usize,
    pub launch_at_login: bool,
    pub share_local_history: bool,
    pub prefer_remote_latest_on_paste: bool,
    pub auto_sync: bool,
    pub discovery_enabled: bool,
    pub hotkey: String,
    pub trusted_peers: Vec<TrustedPeer>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::new(None)
    }
}

impl AppConfig {
    pub fn new(device_name_hint: Option<&str>) -> Self {
        let device_id = Uuid::new_v4().to_string();
        let fallback_name = format!("{APP_NAME} {}", &device_id[..8]);
        let device_name = device_name_hint
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(fallback_name.as_str())
            .to_string();

        Self {
            device_id,
            device_name,
            listen_port: DEFAULT_LISTEN_PORT,
            history_limit: DEFAULT_HISTORY_LIMIT,
            launch_at_login: false,
            share_local_history: true,
            prefer_remote_latest_on_paste: true,
            auto_sync: true,
            discovery_enabled: true,
            hotkey: DEFAULT_HISTORY_POPUP_HOTKEY.to_string(),
            trusted_peers: Vec::new(),
        }
    }

    pub fn normalized(mut self, device_name_hint: Option<&str>) -> Self {
        if self.device_name.trim().is_empty() {
            self.device_name = device_name_hint
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("{APP_NAME} {}", &self.device_id[..8]));
        }
        if self.hotkey.trim().is_empty() {
            self.hotkey = DEFAULT_HISTORY_POPUP_HOTKEY.to_string();
        }
        self.auto_sync = true;
        self
    }
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: std::path::PathBuf,
    cipher: LocalDataCipher,
}

impl ConfigStore {
    pub fn new(paths: &AppPaths, cipher: LocalDataCipher) -> Self {
        Self {
            path: paths.config_file.clone(),
            cipher,
        }
    }

    pub fn load_or_init(&self, device_name_hint: Option<&str>) -> AppResult<AppConfig> {
        if self.path.exists() {
            let (bytes, was_legacy_plaintext) =
                load_or_legacy_bytes(&self.path, CONFIG_PURPOSE, &self.cipher)?;
            let config = serde_json::from_slice(&bytes)?;
            if was_legacy_plaintext {
                self.save(&config)?;
            }
            Ok(config)
        } else {
            let config = AppConfig::new(device_name_hint);
            self.save(&config)?;
            Ok(config)
        }
    }

    pub fn load_or_init_normalized(&self, device_name_hint: Option<&str>) -> AppResult<AppConfig> {
        let config = self.load_or_init(device_name_hint)?;
        let normalized = config.clone().normalized(device_name_hint);
        if normalized != config {
            self.save(&normalized)?;
        }
        Ok(normalized)
    }

    pub fn save(&self, config: &AppConfig) -> AppResult<()> {
        let bytes = serde_json::to_vec_pretty(config)?;
        save_sealed_bytes(&self.path, CONFIG_PURPOSE, &bytes, &self.cipher)
    }
}
