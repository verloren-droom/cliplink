use std::time::Instant;

use super::*;
use crate::core::{
    clipboard::ClipboardBackend, error::AppResult, paths::AppPaths, storage::HistoryStoreProfile,
};

impl AppController {
    pub(crate) fn bootstrap(
        paths: AppPaths,
        clipboard: Box<dyn ClipboardBackend>,
        local_data_cipher: crate::core::at_rest::LocalDataCipher,
        history_store_profile: HistoryStoreProfile,
        runtime_policy: ControllerRuntimePolicy,
        device_name_hint: Option<&str>,
    ) -> AppResult<Self> {
        let config_store = crate::core::config::ConfigStore::new(&paths, local_data_cipher.clone());
        let config = config_store.load_or_init_normalized(device_name_hint)?;
        let poll_interval = clipboard.recommended_poll_interval();

        let history_store = crate::core::storage::HistoryStore::open(
            &paths,
            local_data_cipher.clone(),
            history_store_profile,
        )?;
        let local_history = history_store
            .recent(config.history_limit)?
            .into_iter()
            .map(|item| Self::build_history_entry(&item))
            .collect::<Vec<_>>();
        let now = Instant::now();
        let last_clipboard_poll = now.checked_sub(poll_interval).unwrap_or(now);

        let mut controller = Self {
            services: ControllerServices {
                paths,
                local_data_cipher,
                config_store,
                history_store,
                history_store_profile,
                clipboard,
                runtime: None,
                runtime_policy,
            },
            config,
            local_history,
            remote_history: super::remote::RemoteHistorySession::default(),
            discovered: Vec::new(),
            device_last_seen: std::collections::HashMap::new(),
            remote_share_enabled: std::collections::HashMap::new(),
            status: None,
            last_clipboard_poll,
            preferences_visible: false,
            pending_trust_requests: Vec::new(),
            requested_history_sync_peers: std::collections::HashSet::new(),
            announced_share_state_peers: std::collections::HashSet::new(),
            pending_remote_activation: None,
            active_transfer: None,
        };
        controller.sync_runtime()?;
        Ok(controller)
    }
}
