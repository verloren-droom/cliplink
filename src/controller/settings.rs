use super::*;

use crate::{
    constants::{
        config::DEFAULT_HISTORY_POPUP_HOTKEY,
        limits::{MAX_DEVICE_NAME_CHARS, MAX_HISTORY_LIMIT},
    },
    core::error::{AppError, AppResult},
};

impl AppController {
    pub(crate) fn hotkey(&self) -> &str {
        if self.config.hotkey.trim().is_empty() {
            DEFAULT_HISTORY_POPUP_HOTKEY
        } else {
            self.config.hotkey.as_str()
        }
    }

    pub(crate) fn settings_snapshot(&self) -> SettingsSnapshot {
        let current = self.current_settings_update();
        SettingsSnapshot {
            device_name: current.device_name,
            history_limit: current.history_limit,
            hotkey: current.hotkey,
            launch_at_login: current.launch_at_login,
            share_local_history: current.share_local_history,
            prefer_remote_latest_on_paste: current.prefer_remote_latest_on_paste,
            discovery_enabled: current.discovery_enabled,
            devices: self.device_entries(),
            status: self.status.clone(),
        }
    }

    #[cfg_attr(target_os = "android", allow(dead_code))]
    pub(crate) fn settings_device_entries(&self) -> Vec<SettingsDeviceEntry> {
        self.device_entries()
    }

    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub(crate) fn local_device_identity(&self) -> (String, String) {
        (
            self.config.device_id.clone(),
            self.config.device_name.clone(),
        )
    }

    #[cfg_attr(target_os = "android", allow(dead_code))]
    pub(crate) fn current_settings_update(&self) -> SettingsUpdate {
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

    pub(crate) fn normalize_settings_update(&self, mut update: SettingsUpdate) -> SettingsUpdate {
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

    pub(crate) fn validate_settings_update(&self, update: &SettingsUpdate) -> AppResult<()> {
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

        if update.hotkey.trim().is_empty() {
            return Err(AppError::InvalidConfig(
                "History popup hotkey cannot be empty.".to_string(),
            ));
        }

        Ok(())
    }

    pub(crate) fn apply_settings(&mut self, update: SettingsUpdate) -> AppResult<()> {
        let update = self.normalize_settings_update(update);
        self.validate_settings_update(&update)?;

        let mut next = self.config.clone();
        let share_local_history_changed =
            self.config.share_local_history != update.share_local_history;
        let paste_preference_changed =
            self.config.prefer_remote_latest_on_paste != update.prefer_remote_latest_on_paste;
        next.device_name = update.device_name;
        next.history_limit = update.history_limit;
        next.hotkey = update.hotkey;
        next.launch_at_login = update.launch_at_login;
        next.share_local_history = update.share_local_history;
        next.prefer_remote_latest_on_paste = update.prefer_remote_latest_on_paste;
        next.discovery_enabled = update.discovery_enabled;

        self.services.config_store.save(&next)?;
        let pruned_ids = self
            .services
            .history_store
            .enforce_limit(next.history_limit)?;
        self.config = next;
        self.reload_local_history_cache()?;

        let runtime_sync_error = self.sync_runtime().err();
        let history_prune_sync_error = if !pruned_ids.is_empty() && self.config.share_local_history
        {
            self.broadcast_history_removals(pruned_ids).err()
        } else {
            None
        };
        let share_state_error = if share_local_history_changed {
            self.notify_share_state(self.config.share_local_history)
                .err()
        } else {
            None
        };
        let clipboard_sync_error = if paste_preference_changed {
            self.sync_preferred_latest_to_clipboard().err()
        } else {
            None
        };

        let mut status = "设置已保存".to_string();
        if let Some(error) = runtime_sync_error {
            status.push_str(&format!("，但后台网络服务更新失败: {error}"));
        }
        if let Some(error) = history_prune_sync_error {
            status.push_str(&format!("，但同步裁剪后的历史删除失败: {error}"));
        }
        if let Some(error) = share_state_error {
            status.push_str(&format!("，但同步共享状态失败: {error}"));
        }
        if let Some(error) = clipboard_sync_error {
            status.push_str(&format!("，但更新系统剪切板失败: {error}"));
        }
        self.set_status(status);
        Ok(())
    }

    #[cfg_attr(target_os = "android", allow(dead_code))]
    pub(crate) fn set_preferences_visible(&mut self, visible: bool) -> AppResult<()> {
        if self.preferences_visible == visible {
            return Ok(());
        }

        self.preferences_visible = visible;
        self.sync_runtime()
    }

    #[cfg_attr(target_os = "android", allow(dead_code))]
    pub(crate) fn launch_at_login_enabled(&self) -> bool {
        self.config.launch_at_login
    }
}
