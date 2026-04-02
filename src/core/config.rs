use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

use crate::{
    constants::{
        app::APP_NAME,
        config::{DEFAULT_HISTORY_LIMIT, DEFAULT_HISTORY_POPUP_HOTKEY, DEFAULT_LISTEN_PORT},
        crypto::CONFIG_PURPOSE,
    },
    core::{
        at_rest::{LocalDataCipher, load_sealed_bytes, save_sealed_bytes},
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
        let fallback_name = generated_default_device_name(&device_id);
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
            discovery_enabled: true,
            hotkey: DEFAULT_HISTORY_POPUP_HOTKEY.to_string(),
            trusted_peers: Vec::new(),
        }
    }

    pub fn normalized(mut self, device_name_hint: Option<&str>) -> Self {
        let generated_default_name = generated_default_device_name(&self.device_id);
        if self.device_name.trim().is_empty() || self.device_name.trim() == generated_default_name {
            self.device_name = device_name_hint
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .unwrap_or(generated_default_name);
        }
        if self.hotkey.trim().is_empty() {
            self.hotkey = DEFAULT_HISTORY_POPUP_HOTKEY.to_string();
        }
        normalize_trusted_peers(&mut self.trusted_peers);
        self
    }
}

fn generated_default_device_name(device_id: &str) -> String {
    let suffix = device_id.chars().take(8).collect::<String>();
    format!("{APP_NAME} {suffix}")
}

fn normalize_trusted_peers(trusted_peers: &mut Vec<TrustedPeer>) {
    let mut collapsed = BTreeMap::<String, TrustedPeer>::new();

    for peer in trusted_peers.drain(..).map(normalize_trusted_peer) {
        if peer.device_id.is_empty() && peer.fingerprint.is_empty() {
            continue;
        }

        let key = if peer.fingerprint.is_empty() {
            format!("id:{}", peer.device_id)
        } else {
            format!("fp:{}", peer.fingerprint)
        };

        match collapsed.get_mut(&key) {
            Some(existing) => merge_trusted_peer(existing, peer),
            None => {
                collapsed.insert(key, peer);
            }
        }
    }

    *trusted_peers = collapsed.into_values().collect();
    trusted_peers.sort_by(|left, right| {
        left.device_name
            .cmp(&right.device_name)
            .then_with(|| left.device_id.cmp(&right.device_id))
    });
}

fn normalize_trusted_peer(peer: TrustedPeer) -> TrustedPeer {
    TrustedPeer {
        device_id: peer.device_id.trim().to_string(),
        device_name: peer.device_name.trim().to_string(),
        fingerprint: peer.fingerprint.trim().to_string(),
    }
}

fn merge_trusted_peer(existing: &mut TrustedPeer, candidate: TrustedPeer) {
    let existing_score = trusted_peer_completeness(existing);
    let candidate_score = trusted_peer_completeness(&candidate);

    if existing.device_id.is_empty() && !candidate.device_id.is_empty() {
        existing.device_id = candidate.device_id.clone();
    }
    if existing.device_name.is_empty() && !candidate.device_name.is_empty() {
        existing.device_name = candidate.device_name.clone();
    }
    if existing.fingerprint.is_empty() && !candidate.fingerprint.is_empty() {
        existing.fingerprint = candidate.fingerprint.clone();
    }

    if candidate_score < existing_score {
        return;
    }

    if !candidate.device_id.is_empty() {
        existing.device_id = candidate.device_id;
    }
    if !candidate.device_name.is_empty() {
        existing.device_name = candidate.device_name;
    }
    if !candidate.fingerprint.is_empty() {
        existing.fingerprint = candidate.fingerprint;
    }
}

fn trusted_peer_completeness(peer: &TrustedPeer) -> u8 {
    u8::from(!peer.device_id.is_empty())
        + u8::from(!peer.device_name.is_empty())
        + u8::from(!peer.fingerprint.is_empty())
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
            match load_sealed_bytes(&self.path, CONFIG_PURPOSE, &self.cipher)
                .and_then(|bytes| serde_json::from_slice(&bytes).map_err(Into::into))
            {
                Ok(config) => Ok(config),
                Err(_) => {
                    // Legacy / corrupted config is not supported anymore.
                    // Reset to a fresh encrypted config to avoid startup failure loops.
                    let _ = std::fs::remove_file(&self.path);
                    let config = AppConfig::new(device_name_hint);
                    self.save(&config)?;
                    Ok(config)
                }
            }
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

    #[test]
    fn app_config_normalized_deduplicates_trusted_peers_by_fingerprint() {
        let config = AppConfig {
            device_id: "local-device".to_string(),
            device_name: String::new(),
            listen_port: DEFAULT_LISTEN_PORT,
            history_limit: DEFAULT_HISTORY_LIMIT,
            launch_at_login: false,
            share_local_history: true,
            prefer_remote_latest_on_paste: true,
            discovery_enabled: true,
            hotkey: String::new(),
            trusted_peers: vec![
                trusted_peer("old-id", "HUAWEI NOH-AN01", "fp-1"),
                trusted_peer("new-id", "HUAWEI NOH-AN01", "fp-1"),
                trusted_peer("peer-b", "Pixel 9", "fp-2"),
            ],
        };

        let normalized = config.normalized(Some("Local Mac"));

        assert_eq!(normalized.device_name, "Local Mac");
        assert_eq!(normalized.hotkey, DEFAULT_HISTORY_POPUP_HOTKEY);
        assert_eq!(normalized.trusted_peers.len(), 2);
        assert_eq!(normalized.trusted_peers[0].device_id, "new-id");
        assert_eq!(normalized.trusted_peers[0].fingerprint, "fp-1");
        assert_eq!(normalized.trusted_peers[1].device_id, "peer-b");
        assert_eq!(normalized.trusted_peers[1].fingerprint, "fp-2");
    }

    #[test]
    fn app_config_normalized_drops_blank_trusted_peer_records() {
        let config = AppConfig {
            trusted_peers: vec![
                trusted_peer("   ", "   ", "   "),
                trusted_peer("peer-a", "Phone", ""),
            ],
            ..AppConfig::new(Some("Local Mac"))
        };

        let normalized = config.normalized(Some("Local Mac"));

        assert_eq!(normalized.trusted_peers.len(), 1);
        assert_eq!(normalized.trusted_peers[0].device_id, "peer-a");
        assert_eq!(normalized.trusted_peers[0].device_name, "Phone");
    }
}
