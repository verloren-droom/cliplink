use super::*;

impl AppDelegate {
    pub(in crate::platform::macos) fn activate_selected(&self) {
        let Some(row) = self
            .ivars()
            .selected_row
            .borrow()
            .as_ref()
            .copied()
            .or_else(|| self.current_selected_history_rows().last().copied())
        else {
            return;
        };

        self.activate_row(row);
    }

    pub(in crate::platform::macos) fn activate_clicked_or_selected(
        &self,
        sender: Option<&AnyObject>,
    ) {
        let clicked_row = sender
            .map(|sender| unsafe { msg_send![sender, clickedRow] })
            .filter(|row: &NSInteger| *row >= 0)
            .map(|row| row as usize);

        let row = clicked_row.or_else(|| self.ivars().selected_row.borrow().as_ref().copied());
        let Some(row) = row else {
            return;
        };

        if clicked_row.is_some() {
            let selected_rows = self.current_selected_history_rows();
            if selected_rows.contains(&row) {
                self.ivars().selected_row.replace(Some(row));
            } else {
                self.set_selected_row(row, sender.is_none());
            }
        } else {
            self.set_selected_row(row, sender.is_none());
        }

        if sender.is_some() && !self.current_table_click_is_double_click() {
            return;
        }
        self.activate_row(row);
    }

    fn current_table_click_is_double_click(&self) -> bool {
        let app = NSApplication::sharedApplication(self.mtm());
        app.currentEvent().is_some_and(|event| {
            matches!(
                event.r#type(),
                NSEventType::LeftMouseDown | NSEventType::LeftMouseUp
            ) && event.clickCount() >= 2
        })
    }

    fn activate_row(&self, row: usize) {
        let Some(id) = self
            .ivars()
            .filtered_rows
            .borrow()
            .get(row)
            .map(|row| row.id)
        else {
            return;
        };

        let activation = self.with_controller_mut(|controller| controller.copy_item(id));
        match activation {
            Ok(crate::controller::HistoryActivation::ClipboardReady) => {
                self.hide_panel(true);
                self.trigger_immediate_paste();
            }
            Ok(crate::controller::HistoryActivation::PendingTransfer) => {
                self.refresh_panel_feedback_state();
            }
            Ok(crate::controller::HistoryActivation::Noop) | Err(_) => {}
        }
    }

    pub(in crate::platform::macos) fn delete_history_rows(&self, rows: &[usize]) {
        if rows.is_empty() {
            return;
        }

        let ids = {
            let filtered_rows = self.ivars().filtered_rows.borrow();
            rows.iter()
                .filter_map(|row| filtered_rows.get(*row).map(|entry| entry.id))
                .collect::<Vec<_>>()
        };
        if ids.is_empty() {
            return;
        }

        let anchor_row = rows.iter().min().copied();

        let delete_result =
            self.with_controller_mut(|controller| controller.delete_history_items(&ids));
        let deleted = match delete_result {
            Ok(count) => count,
            Err(error) => {
                self.report_controller_status(format!("删除剪切板历史失败: {error}"));
                self.refresh_panel_feedback_state();
                return;
            }
        };
        if deleted == 0 {
            self.refresh_panel_feedback_state();
            return;
        }

        self.reload_filtered_rows();

        let remaining = self.ivars().filtered_rows.borrow().len();
        if remaining == 0 {
            self.clear_history_selection();
            return;
        }

        let target = anchor_row.unwrap_or(0).saturating_sub(1).min(remaining - 1);
        self.set_selected_row(target, true);
    }

    pub(in crate::platform::macos) fn toggle_history_rows_pin(&self, rows: &[usize]) {
        if rows.is_empty() {
            return;
        }

        let selected_items = {
            let filtered_rows = self.ivars().filtered_rows.borrow();
            let ids = rows
                .iter()
                .filter_map(|row| filtered_rows.get(*row).map(|entry| entry.id))
                .collect::<Vec<_>>();
            let controller = self.ivars().controller.borrow();
            ids.into_iter()
                .filter_map(|id| controller.load_item(id).ok().flatten())
                .collect::<Vec<_>>()
        };

        let local_items = selected_items
            .iter()
            .filter(|item| !item.is_remote)
            .collect::<Vec<_>>();
        if local_items.is_empty() {
            self.report_controller_status("远程历史为只读，无法修改锁定状态");
            self.refresh_panel_feedback_state();
            return;
        }
        let next_pinned = !local_items.iter().all(|item| item.is_pinned);
        let ids = local_items.iter().map(|item| item.id).collect::<Vec<_>>();

        let update_result = self.with_controller_mut(|controller| {
            controller.set_history_items_pinned(&ids, next_pinned)
        });
        match update_result {
            Ok(_) => self.reload_filtered_rows_revealing_selection(),
            Err(error) => {
                self.report_controller_status(format!("更新锁定状态失败: {error}"));
                self.refresh_panel_feedback_state();
            }
        }
    }

    pub(in crate::platform::macos) fn open_history_item_parent_folders_for_rows(
        &self,
        rows: &[usize],
    ) {
        if rows.is_empty() {
            return;
        }

        let directories = {
            let filtered_rows = self.ivars().filtered_rows.borrow();
            let ids = rows
                .iter()
                .filter_map(|row| filtered_rows.get(*row).map(|entry| entry.id))
                .collect::<Vec<_>>();
            let controller = self.ivars().controller.borrow();
            let mut directories = BTreeSet::new();
            for id in ids {
                let Ok(Some(item)) = controller.load_item(id) else {
                    continue;
                };
                if item.is_remote {
                    continue;
                }
                for directory in item.local_file_parent_directories() {
                    directories.insert(directory);
                }
            }
            directories
        };

        if directories.is_empty() {
            self.report_controller_status("当前条目没有可打开的本地文件夹");
            self.refresh_panel_feedback_state();
            return;
        }

        let workspace = NSWorkspace::sharedWorkspace();
        let mut opened = 0usize;
        for directory in directories {
            let path = directory.to_string_lossy();
            let url = NSURL::fileURLWithPath(&NSString::from_str(path.as_ref()));
            if workspace.openURL(&url) {
                opened += 1;
            }
        }

        self.report_controller_status(if opened == 0 {
            "未能打开所在文件夹".to_string()
        } else if opened == 1 {
            "已打开所在文件夹".to_string()
        } else {
            format!("已打开 {opened} 个所在文件夹")
        });
        self.refresh_panel_feedback_state();
    }
}
