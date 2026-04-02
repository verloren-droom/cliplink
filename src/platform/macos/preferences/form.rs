use super::*;

impl AppDelegate {
    pub(in crate::platform::macos) fn populate_preferences_window(
        &self,
        status_override: Option<String>,
    ) {
        let controller = self.ivars().controller.borrow();
        let snapshot = controller.settings_snapshot();
        let baseline = controller.current_settings_update();
        drop(controller);

        if let Some(field) = self.preferences_device_name_field() {
            field.setStringValue(&NSString::from_str(&snapshot.device_name));
        }
        if let Some(field) = self.preferences_history_limit_field() {
            field.setIntValue(snapshot.history_limit.min(i32::MAX as usize) as i32);
        }
        if let Some(field) = self.preferences_hotkey_field() {
            field.setStringValue(&NSString::from_str(&format_hotkey_for_display(
                &snapshot.hotkey,
            )));
        }
        self.ivars()
            .preferences_hotkey_value
            .replace(snapshot.hotkey.clone());
        self.refresh_hotkey_capture_ui(false);
        if let Some(button) = self.preferences_launch_at_login_checkbox() {
            button.setIntValue(if snapshot.launch_at_login { 1 } else { 0 });
        }
        if let Some(button) = self.preferences_share_checkbox() {
            button.setIntValue(if snapshot.share_local_history { 1 } else { 0 });
        }
        if let Some(button) = self.preferences_prefer_remote_paste_checkbox() {
            button.setIntValue(if snapshot.prefer_remote_latest_on_paste {
                1
            } else {
                0
            });
        }
        if let Some(button) = self.preferences_discovery_checkbox() {
            button.setIntValue(if snapshot.discovery_enabled { 1 } else { 0 });
        }
        self.replace_preferences_devices(snapshot.devices);

        self.ivars().preferences_baseline.replace(Some(baseline));
        self.sync_preferences_form_state();

        let status = status_override.or(snapshot.status).unwrap_or_default();
        self.set_preferences_status(status);
    }

    pub(in crate::platform::macos) fn refresh_preferences_runtime_state(&self) {
        self.refresh_preferences_devices();
        self.refresh_preferences_status();
        self.refresh_hotkey_capture_ui(self.hotkey_capture_active());
    }

    pub(in crate::platform::macos) fn refresh_preferences_devices(&self) {
        let devices = self.ivars().controller.borrow().settings_device_entries();
        self.replace_preferences_devices(devices);
    }

    pub(in crate::platform::macos) fn refresh_preferences_status(&self) {
        let status = self
            .ivars()
            .controller
            .borrow()
            .status_text()
            .unwrap_or_default();
        self.set_preferences_status(status);
    }

    pub(in crate::platform::macos) fn save_preferences(&self) {
        self.stop_hotkey_capture(false);
        let update = match self.validate_preferences_form() {
            Ok(update) => update,
            Err(message) => {
                self.set_preferences_status(message);
                return;
            }
        };

        let previous_launch_at_login = self.ivars().controller.borrow().launch_at_login_enabled();
        let next_launch_at_login = update.launch_at_login;
        if previous_launch_at_login != next_launch_at_login {
            if let Err(error) = autostart::set_launch_at_login(next_launch_at_login) {
                self.set_preferences_status(format!("更新开机自动启动失败: {error}"));
                return;
            }
        }

        let result = self.with_controller_mut(|controller| controller.apply_settings(update));
        match result {
            Ok(()) => match self.refresh_hotkey_binding() {
                Ok(()) => self.populate_preferences_window(Some("偏好设置已保存。".to_string())),
                Err(error) => self.populate_preferences_window(Some(format!(
                    "偏好设置已保存，但快捷键注册失败: {error}"
                ))),
            },
            Err(error) => {
                if previous_launch_at_login != next_launch_at_login {
                    if let Err(revert_error) =
                        autostart::set_launch_at_login(previous_launch_at_login)
                    {
                        self.set_preferences_status(format!(
                            "保存偏好设置失败: {error}；恢复开机自动启动失败: {revert_error}"
                        ));
                        return;
                    }
                }
                self.set_preferences_status(format!("保存偏好设置失败: {error}"));
            }
        }
    }

    pub(in crate::platform::macos) fn sync_preferences_form_state(&self) {
        if let Some(label) = self.preferences_hotkey_preview_label() {
            label.setStringValue(&NSString::from_str(if self.hotkey_capture_active() {
                HOTKEY_CAPTURE_HINT_ACTIVE
            } else {
                HOTKEY_CAPTURE_HINT_IDLE
            }));
        }

        if let Some(field) = self.preferences_hotkey_field() {
            let hotkey = self.ivars().preferences_hotkey_value.borrow();
            field.setStringValue(&NSString::from_str(&format_hotkey_for_display(
                hotkey.as_str(),
            )));
        }

        let validation = self.validate_preferences_form();
        let is_dirty = match validation.as_ref() {
            Ok(update) => self
                .ivars()
                .preferences_baseline
                .borrow()
                .as_ref()
                .map(|baseline| baseline != update)
                .unwrap_or(false),
            Err(_) => false,
        };

        if let Some(button) = self.preferences_save_button() {
            button.setEnabled(is_dirty);
        }

        match validation {
            Ok(_) if is_dirty => self.set_preferences_status(""),
            Ok(_) => {}
            Err(message) => self.set_preferences_status(message),
        }
    }

    fn validate_preferences_form(&self) -> Result<SettingsUpdate, String> {
        let missing_controls = "偏好设置控件当前不可用。".to_string();
        let device_name = self
            .preferences_device_name_field()
            .ok_or_else(|| missing_controls.clone())?
            .stringValue()
            .to_string();
        let device_name = device_name.trim();
        if device_name.is_empty() {
            return Err("设备名称不能为空。".to_string());
        }
        if device_name.chars().count() > MAX_DEVICE_NAME_CHARS {
            return Err(format!("设备名称不能超过 {MAX_DEVICE_NAME_CHARS} 个字符。"));
        }
        if device_name.chars().any(char::is_control) {
            return Err("设备名称不能包含控制字符。".to_string());
        }

        let history_limit_raw = self
            .preferences_history_limit_field()
            .ok_or_else(|| missing_controls.clone())?
            .stringValue()
            .to_string();
        let history_limit = history_limit_raw
            .trim()
            .parse::<usize>()
            .map_err(|_| format!("历史数量上限必须是 1 到 {MAX_HISTORY_LIMIT} 的整数。"))?;
        if !(1..=MAX_HISTORY_LIMIT).contains(&history_limit) {
            return Err(format!(
                "历史数量上限必须在 1 到 {MAX_HISTORY_LIMIT} 之间。"
            ));
        }

        let hotkey = self.ivars().preferences_hotkey_value.borrow().clone();
        if hotkey.trim().is_empty() {
            return Err("历史弹窗快捷键不能为空。".to_string());
        }
        hotkey
            .parse::<HotKey>()
            .map_err(|_| "历史弹窗快捷键无效。".to_string())?;

        let update = SettingsUpdate {
            device_name: device_name.to_string(),
            history_limit,
            hotkey,
            launch_at_login: self
                .preferences_launch_at_login_checkbox()
                .ok_or_else(|| missing_controls.clone())?
                .intValue()
                != 0,
            share_local_history: self
                .preferences_share_checkbox()
                .ok_or_else(|| missing_controls.clone())?
                .intValue()
                != 0,
            prefer_remote_latest_on_paste: self
                .preferences_prefer_remote_paste_checkbox()
                .ok_or_else(|| missing_controls.clone())?
                .intValue()
                != 0,
            discovery_enabled: self
                .preferences_discovery_checkbox()
                .ok_or_else(|| missing_controls.clone())?
                .intValue()
                != 0,
        };

        let normalized = self
            .ivars()
            .controller
            .borrow()
            .normalize_settings_update(update);
        self.ivars()
            .controller
            .borrow()
            .validate_settings_update(&normalized)
            .map_err(|error| error.to_string())?;
        Ok(normalized)
    }

    pub(in crate::platform::macos) fn set_preferences_status(&self, message: impl Into<String>) {
        let Some(label) = self.preferences_status_label() else {
            return;
        };
        label.setStringValue(&NSString::from_str(&message.into()));
    }
}
