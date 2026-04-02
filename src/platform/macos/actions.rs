use objc2::{DefinedClass, MainThreadOnly, msg_send, rc::autoreleasepool, runtime::AnyObject};
use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn};
use objc2_foundation::{NSInteger, NSString, NSTimer};

use crate::controller::{HistoryActivation, HistoryScope};

use super::AppDelegate;

impl AppDelegate {
    pub(super) fn perform_status_item_action(&self) {
        self.handle_status_item_click();
    }

    pub(super) fn perform_tick(&self, _timer: &NSTimer) {
        autoreleasepool(|_| {
            while let Ok(event) = global_hotkey::GlobalHotKeyEvent::receiver().try_recv() {
                if event.state == global_hotkey::HotKeyState::Released {
                    self.toggle_panel();
                }
            }

            let outcome = {
                let mut controller = self.ivars().controller.borrow_mut();
                controller.tick()
            };

            if (outcome.history_changed || outcome.devices_changed) && self.panel_visible() {
                self.reload_filtered_rows();
            }
            if self.panel_visible() && (outcome.status_changed || outcome.transfer_changed) {
                self.refresh_panel_feedback_state();
            }
            if outcome.paste_requested {
                if self.panel_visible() {
                    self.hide_panel(true);
                }
                self.trigger_immediate_paste();
            }
            self.present_pending_trust_prompt_if_needed();
            if self
                .preferences_window()
                .is_some_and(|window| window.isVisible())
            {
                if outcome.devices_changed {
                    self.refresh_preferences_devices();
                }
                if outcome.status_changed {
                    self.refresh_preferences_status();
                }
            }
        });
    }

    pub(super) fn perform_activate_selected_action(&self, sender: Option<&AnyObject>) {
        self.activate_clicked_or_selected(sender);
    }

    pub(super) fn perform_delete_history_item_action(&self, sender: Option<&AnyObject>) {
        let resolved_rows = self.resolve_history_rows_action_target(sender);
        let rows = self.local_rows_for_batch_actions(resolved_rows.as_slice());
        if !rows.is_empty() {
            self.delete_history_rows(rows.as_slice());
        }
    }

    pub(super) fn perform_toggle_history_item_pin_action(&self, sender: Option<&AnyObject>) {
        let resolved_rows = self.resolve_history_rows_action_target(sender);
        let rows = self.local_rows_for_batch_actions(resolved_rows.as_slice());
        if rows.is_empty() {
            return;
        }
        self.toggle_history_rows_pin(rows.as_slice());
    }

    pub(super) fn perform_open_history_item_parent_folders_action(
        &self,
        sender: Option<&AnyObject>,
    ) {
        let resolved_rows = self.resolve_history_rows_action_target(sender);
        let rows = self.local_rows_for_batch_actions(resolved_rows.as_slice());
        if rows.is_empty() {
            return;
        }
        self.open_history_item_parent_folders_for_rows(rows.as_slice());
    }

    pub(super) fn perform_clear_history_action(&self) {
        let Some(include_pinned) = self.confirm_clear_history() else {
            return;
        };
        let clear_result = self.with_controller_mut(|controller| {
            controller.clear_history_with_options(include_pinned)
        });
        if let Err(error) = clear_result {
            self.report_controller_status(format!("清除剪切板历史失败: {error}"));
        }
        self.reload_filtered_rows();
        self.refresh_preferences_runtime_state();
    }

    pub(super) fn perform_open_preferences_action(&self) {
        self.show_preferences_window();
    }

    pub(super) fn perform_open_about_action(&self) {
        self.show_about_window();
    }

    pub(super) fn perform_history_scope_changed_action(&self) {
        let Some(scope_button) = self.history_scope_button() else {
            return;
        };

        let selected_index = scope_button.indexOfSelectedItem();
        let next_scope_key = self
            .ivars()
            .history_scope_options
            .borrow()
            .get(selected_index.max(0) as usize)
            .map(|option| option.key.clone())
            .unwrap_or_else(|| HistoryScope::All.key());
        self.ivars().history_scope_key.replace(next_scope_key);
        self.reload_filtered_rows_revealing_selection();
    }

    pub(super) fn perform_close_about_action(&self) {
        self.hide_about_window();
    }

    pub(super) fn perform_activate_menu_history_action(&self, sender: Option<&AnyObject>) {
        let Some(sender) = sender else {
            return;
        };
        let tag: NSInteger = unsafe { msg_send![sender, tag] };
        let Some(id) = self
            .ivars()
            .menu_history_ids
            .borrow()
            .get(tag.max(0) as usize)
            .copied()
        else {
            return;
        };

        let activation = self.with_controller_mut(|controller| controller.copy_item(id));
        match activation {
            Ok(HistoryActivation::ClipboardReady) => self.trigger_immediate_paste(),
            Ok(HistoryActivation::PendingTransfer | HistoryActivation::Noop) | Err(_) => {}
        }
    }

    pub(super) fn perform_save_preferences_action(&self) {
        self.save_preferences();
    }

    pub(super) fn perform_close_preferences_action(&self) {
        self.hide_preferences_window();
    }

    pub(super) fn perform_preferences_changed_action(&self) {
        self.sync_preferences_form_state();
    }

    pub(super) fn perform_toggle_hotkey_capture_action(&self) {
        self.toggle_hotkey_capture();
    }

    pub(super) fn perform_trust_selected_device_action(&self) {
        let device_ids = self.selected_preferences_device_ids();
        if device_ids.is_empty() {
            self.set_preferences_status("请选择至少一个在线设备。");
            return;
        }

        let result = self
            .with_controller_mut(|controller| controller.request_device_trust_many(&device_ids));
        match result {
            Ok(_) => self.refresh_preferences_runtime_state(),
            Err(error) => self.set_preferences_status(format!("信任设备失败: {error}")),
        }
    }

    pub(super) fn perform_revoke_selected_device_action(&self) {
        let entries = self.selected_preferences_device_entries();
        if entries.is_empty() {
            self.set_preferences_status("请选择至少一个已信任设备。");
            return;
        }
        let trusted_entries = entries
            .into_iter()
            .filter(|entry| entry.is_trusted)
            .collect::<Vec<_>>();
        if trusted_entries.is_empty() {
            self.set_preferences_status("请选择至少一个已信任设备。");
            return;
        }
        if !self.confirm_revoke_trusted_devices(trusted_entries.as_slice()) {
            return;
        }

        let device_ids = trusted_entries
            .iter()
            .map(|entry| entry.device_id.clone())
            .collect::<Vec<_>>();

        let result =
            self.with_controller_mut(|controller| controller.revoke_device_trust_many(&device_ids));
        match result {
            Ok(count) if count > 0 => self.refresh_preferences_runtime_state(),
            Ok(_) => self.set_preferences_status("未找到对应的信任设备。"),
            Err(error) => self.set_preferences_status(format!("移除信任设备失败: {error}")),
        }
    }

    pub(super) fn perform_show_selected_device_properties_action(&self) {
        self.show_selected_device_properties();
    }

    pub(super) fn perform_quit_app_action(&self) {
        let app = objc2_app_kit::NSApplication::sharedApplication(self.mtm());
        app.terminate(None);
    }

    fn resolve_history_rows_action_target(&self, sender: Option<&AnyObject>) -> Vec<usize> {
        let target_row = sender
            .map(|control| unsafe { msg_send![control, tag] })
            .filter(|tag: &NSInteger| *tag >= 0)
            .map(|tag| tag as usize)
            .or_else(|| self.context_history_row())
            .or_else(|| self.ivars().selected_row.borrow().as_ref().copied());

        let mut selected_rows = self.current_selected_history_rows();
        match target_row {
            Some(row) if selected_rows.contains(&row) => {
                self.ivars().selected_row.replace(Some(row));
                if selected_rows.is_empty() {
                    selected_rows.push(row);
                }
                selected_rows
            }
            Some(row) => {
                self.set_selected_row(row, false);
                vec![row]
            }
            None => selected_rows,
        }
    }

    fn confirm_revoke_trusted_devices(
        &self,
        entries: &[crate::controller::SettingsDeviceEntry],
    ) -> bool {
        if entries.is_empty() {
            return false;
        }

        let informative_text = if entries.len() == 1 {
            format!(
                "将移除与“{}”的信任关系，并通知对方同步解除信任。\n\n移除后需重新发起连接请求才能再次共享。",
                entries[0].device_name
            )
        } else {
            let device_list = entries
                .iter()
                .take(3)
                .map(|entry| format!("• {}", entry.device_name))
                .collect::<Vec<_>>()
                .join("\n");
            let suffix = if entries.len() > 3 {
                format!("\n…以及另外 {} 台设备", entries.len() - 3)
            } else {
                String::new()
            };
            format!(
                "将移除 {count} 台设备的信任关系，并通知对方同步解除信任。\n\n{device_list}{suffix}\n\n移除后需重新发起连接请求才能再次共享。",
                count = entries.len(),
                device_list = device_list,
                suffix = suffix,
            )
        };

        let alert = NSAlert::new(self.mtm());
        alert.setMessageText(&NSString::from_str("确认移除信任设备"));
        alert.setInformativeText(&NSString::from_str(&informative_text));
        alert.addButtonWithTitle(&NSString::from_str("移除信任"));
        alert.addButtonWithTitle(&NSString::from_str("取消"));
        alert.runModal() == NSAlertFirstButtonReturn
    }
}
