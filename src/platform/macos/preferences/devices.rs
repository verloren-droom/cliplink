use super::*;

impl AppDelegate {
    pub(in crate::platform::macos) fn sync_selected_device_from_table(&self) {
        let Some(table_view) = self.preferences_devices_table() else {
            return;
        };
        let rows = selected_rows_from_table(&table_view);
        let selected_device_ids = self.preference_device_ids_for_rows(rows.as_slice());
        let selected_device_id = rows
            .last()
            .and_then(|row| self.preference_device_id_for_row(*row));
        self.ivars()
            .preferences_selected_device_id
            .replace(selected_device_id);
        self.ivars()
            .preferences_selected_device_ids
            .replace(selected_device_ids);
        self.refresh_preferences_device_actions();
    }

    pub(in crate::platform::macos) fn replace_preferences_devices(
        &self,
        devices: Vec<crate::controller::SettingsDeviceEntry>,
    ) {
        let previous_selection = self.selected_preferences_device_ids();
        let previous_primary_selection =
            self.ivars().preferences_selected_device_id.borrow().clone();
        *self.ivars().preferences_devices_rows.borrow_mut() = devices;

        let matched_rows = {
            let rows = self.ivars().preferences_devices_rows.borrow();
            let mut matched = previous_selection
                .iter()
                .filter_map(|selected| rows.iter().position(|entry| &entry.device_id == selected))
                .collect::<Vec<_>>();
            if matched.is_empty() && !rows.is_empty() {
                matched.push(0);
            }
            matched
        };
        let primary_row = {
            let rows = self.ivars().preferences_devices_rows.borrow();
            previous_primary_selection
                .as_ref()
                .and_then(|selected| rows.iter().position(|entry| &entry.device_id == selected))
                .or_else(|| matched_rows.last().copied())
        };

        let Some(table_view) = self.preferences_devices_table() else {
            let selected_ids = self.preference_device_ids_for_rows(matched_rows.as_slice());
            let primary_id = primary_row.and_then(|row| self.preference_device_id_for_row(row));
            self.ivars()
                .preferences_selected_device_ids
                .replace(selected_ids);
            self.ivars()
                .preferences_selected_device_id
                .replace(primary_id);
            self.refresh_preferences_device_actions();
            return;
        };

        table_view.reloadData();

        if !matched_rows.is_empty() {
            self.set_selected_preferences_rows(matched_rows.as_slice(), primary_row, true);
        } else {
            self.ivars().preferences_selected_device_id.replace(None);
            self.ivars()
                .preferences_selected_device_ids
                .replace(Vec::new());
            unsafe {
                table_view.deselectAll(None);
            }
        }

        self.refresh_preferences_device_actions();
    }

    pub(in crate::platform::macos) fn refresh_preferences_device_actions(&self) {
        if let Some(table_view) = self.preferences_devices_table() {
            let clicked_row = table_view.clickedRow();
            if clicked_row >= 0 {
                let row_index = clicked_row as usize;
                let selected_rows = selected_rows_from_table(&table_view);
                if !selected_rows.contains(&row_index) {
                    self.set_selected_preferences_rows(&[row_index], Some(row_index), false);
                } else {
                    self.ivars()
                        .preferences_selected_device_id
                        .replace(self.preference_device_id_for_row(row_index));
                }
            }
        }

        let selected_entries = self.selected_preferences_device_entries();

        let Some(item) = self.preferences_devices_context_action_item() else {
            return;
        };
        if let Some(properties_item) = self.preferences_devices_context_properties_item() {
            properties_item.setEnabled(selected_entries.len() == 1);
        }
        if selected_entries.is_empty() {
            item.setEnabled(false);
            item.setTitle(&NSString::from_str("信任设备"));
            unsafe {
                item.setAction(Some(sel!(trustSelectedDevice:)));
            }
            return;
        }

        let all_trusted = selected_entries.iter().all(|entry| entry.is_trusted);
        let all_untrusted = selected_entries.iter().all(|entry| !entry.is_trusted);

        if all_trusted {
            item.setEnabled(true);
            item.setTitle(&NSString::from_str("移除信任"));
            unsafe {
                item.setAction(Some(sel!(revokeSelectedDevice:)));
            }
        } else if all_untrusted {
            item.setEnabled(selected_entries.iter().all(|entry| entry.is_online));
            item.setTitle(&NSString::from_str("信任设备"));
            unsafe {
                item.setAction(Some(sel!(trustSelectedDevice:)));
            }
        } else {
            item.setEnabled(false);
            item.setTitle(&NSString::from_str("信任设备"));
            unsafe {
                item.setAction(Some(sel!(trustSelectedDevice:)));
            }
        }
    }

    pub(in crate::platform::macos) fn selected_preferences_device_ids(&self) -> Vec<String> {
        let selected_ids = self
            .ivars()
            .preferences_selected_device_ids
            .borrow()
            .clone();
        if !selected_ids.is_empty() {
            return selected_ids;
        }

        self.ivars()
            .preferences_selected_device_id
            .borrow()
            .clone()
            .map(|device_id| vec![device_id])
            .unwrap_or_default()
    }

    pub(in crate::platform::macos) fn selected_preferences_device_entries(
        &self,
    ) -> Vec<crate::controller::SettingsDeviceEntry> {
        let selected_ids = self.selected_preferences_device_ids();
        let rows = self.ivars().preferences_devices_rows.borrow();
        selected_ids
            .into_iter()
            .filter_map(|selected_id| {
                rows.iter()
                    .find(|entry| entry.device_id == selected_id)
                    .cloned()
            })
            .collect()
    }

    pub(in crate::platform::macos) fn show_selected_device_properties(&self) {
        let selected_entries = self.selected_preferences_device_entries();
        if selected_entries.is_empty() {
            self.set_preferences_status("请先选择设备。");
            return;
        }
        if selected_entries.len() > 1 {
            self.set_preferences_status("设备属性仅支持查看单个设备。");
            return;
        }
        let entry = &selected_entries[0];

        let status_text = crate::controller::AppController::device_status_label(entry.status_kind);

        let mut detail = format!(
            "设备名称: {}\n设备 ID: {}\n状态: {}\n地址: {}",
            entry.device_name, entry.device_id, status_text, entry.secondary_text
        );
        if !entry.status_tooltip.trim().is_empty() {
            detail.push_str("\n\n");
            detail.push_str(entry.status_tooltip.trim());
        }

        let alert = NSAlert::new(self.mtm());
        alert.setMessageText(&NSString::from_str("设备属性"));
        alert.setInformativeText(&NSString::from_str(&detail));
        alert.addButtonWithTitle(&NSString::from_str("关闭"));
        let _ = alert.runModal();
    }

    fn preference_device_id_for_row(&self, row: usize) -> Option<String> {
        self.ivars()
            .preferences_devices_rows
            .borrow()
            .get(row)
            .map(|entry| entry.device_id.clone())
    }

    fn preference_device_ids_for_rows(&self, rows: &[usize]) -> Vec<String> {
        let device_rows = self.ivars().preferences_devices_rows.borrow();
        rows.iter()
            .filter_map(|row| device_rows.get(*row).map(|entry| entry.device_id.clone()))
            .collect()
    }

    fn set_selected_preferences_rows(
        &self,
        rows: &[usize],
        primary_row: Option<usize>,
        reveal: bool,
    ) {
        let Some(table_view) = self.preferences_devices_table() else {
            return;
        };
        let indexes = index_set_from_rows(rows);
        let selected_ids = self.preference_device_ids_for_rows(rows);
        let primary_id = primary_row.and_then(|row| self.preference_device_id_for_row(row));
        self.ivars()
            .preferences_selected_device_ids
            .replace(selected_ids);
        self.ivars()
            .preferences_selected_device_id
            .replace(primary_id);
        table_view.selectRowIndexes_byExtendingSelection(&indexes, false);
        if reveal {
            if let Some(row) = primary_row {
                table_view.scrollRowToVisible(row as NSInteger);
            } else if let Some(row) = rows.first().copied() {
                table_view.scrollRowToVisible(row as NSInteger);
            }
        }
    }
}
