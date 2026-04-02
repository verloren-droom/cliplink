use super::*;

impl AppDelegate {
    pub(in crate::platform::macos) fn reload_filtered_rows(&self) {
        self.reload_filtered_rows_with_options(false);
    }

    pub(in crate::platform::macos) fn reload_filtered_rows_revealing_selection(&self) {
        self.reload_filtered_rows_with_options(true);
    }

    fn reload_filtered_rows_with_options(&self, reveal_selection: bool) {
        let selected_ids = self.current_selected_history_ids();
        let selected_id = self.current_selected_history_id();
        let previous_scroll_origin = self.current_scroll_origin();
        let query = self.ivars().search_query.borrow().clone();
        let scope = self.current_history_scope();
        let (options, normalized_scope_key, rows) = self.with_controller(|controller| {
            let normalized_scope = controller.normalize_history_scope(scope);
            let normalized_scope_key = normalized_scope.key();
            let options = controller.history_scope_options();
            let rows = controller.history_rows_with_scope(&query, normalized_scope);
            (options, normalized_scope_key, rows)
        });
        self.refresh_history_scope_button(options, normalized_scope_key);
        let matched_rows = selected_ids
            .iter()
            .filter_map(|id| rows.iter().position(|row| row.id == *id))
            .collect::<Vec<_>>();
        let matched_selection = selected_id.and_then(|id| rows.iter().position(|row| row.id == id));
        *self.ivars().filtered_rows.borrow_mut() = rows;
        let matched_ids = self.history_ids_for_rows(matched_rows.as_slice());
        self.ivars().selected_history_ids.replace(matched_ids);
        self.ivars().selected_row.replace(matched_selection);

        let Some(table_view) = self.table_view() else {
            return;
        };

        table_view.reloadData();
        let selected_rows = self.current_selected_history_rows();
        let selected_row = self.ivars().selected_row.borrow().as_ref().copied();
        let row_count = self.ivars().filtered_rows.borrow().len();
        let should_reveal_selection = reveal_selection
            || selected_rows.is_empty()
            || selected_row.is_none()
            || selected_row.is_some_and(|row| row >= row_count);
        self.ensure_selection(should_reveal_selection);
        if !should_reveal_selection {
            self.restore_scroll_origin(previous_scroll_origin);
        }
    }

    fn refresh_history_scope_button(
        &self,
        options: Vec<HistoryScopeOption>,
        normalized_key: String,
    ) {
        let Some(scope_button) = self.history_scope_button() else {
            return;
        };

        scope_button.removeAllItems();
        for option in &options {
            scope_button.addItemWithTitle(&NSString::from_str(&option.label));
        }

        let selected_index = options
            .iter()
            .position(|option| option.key == normalized_key)
            .unwrap_or(0);
        scope_button.selectItemAtIndex(selected_index as NSInteger);

        self.ivars().history_scope_key.replace(normalized_key);
        self.ivars().history_scope_options.replace(options);
    }

    pub(in crate::platform::macos) fn current_history_scope(&self) -> HistoryScope {
        HistoryScope::from_raw(&self.ivars().history_scope_key.borrow())
    }

    pub(in crate::platform::macos) fn refresh_panel_feedback_state(&self) {
        let (status_text, transfer_progress) = {
            let controller = self.ivars().controller.borrow();
            (
                controller.status_text(),
                controller.transfer_progress_snapshot(),
            )
        };

        if let Some(label) = self.panel_status_label() {
            let message = transfer_progress
                .as_ref()
                .map(|progress| progress.label.clone())
                .or(status_text)
                .unwrap_or_default();
            label.setHidden(message.trim().is_empty());
            label.setStringValue(&NSString::from_str(&message));
            if let Some(progress) = transfer_progress.as_ref() {
                let tooltip = NSString::from_str(&progress.detail);
                label.setToolTip(Some(&tooltip));
            } else if !message.trim().is_empty() {
                label.setToolTip(Some(&NSString::from_str(&message)));
            } else {
                label.setToolTip(None);
            }
        }

        let Some(track) = self.panel_progress_track() else {
            return;
        };
        let Some(fill) = self.panel_progress_fill() else {
            return;
        };

        let Some(progress) = transfer_progress else {
            track.setHidden(true);
            fill.setHidden(true);
            return;
        };

        let track_frame = track.frame();
        let fill_width = (track_frame.size.width * progress.fraction.clamp(0.0, 1.0)).max(0.0);
        fill.setFrame(NSRect::new(
            track_frame.origin,
            NSSize::new(fill_width, track_frame.size.height),
        ));
        let tooltip = NSString::from_str(&progress.detail);
        track.setToolTip(Some(&tooltip));
        fill.setToolTip(Some(&tooltip));
        track.setHidden(false);
        fill.setHidden(false);
    }

    fn ensure_selection(&self, scroll: bool) {
        let Some(table_view) = self.table_view() else {
            return;
        };
        let rows = self.ivars().filtered_rows.borrow().len();
        if rows == 0 {
            self.ivars().selected_row.replace(None);
            self.ivars().selected_history_ids.replace(Vec::new());
            unsafe {
                table_view.deselectAll(None);
            }
            return;
        }

        let selected_rows = self
            .current_selected_history_rows()
            .into_iter()
            .filter(|row| *row < rows)
            .collect::<Vec<_>>();
        if selected_rows.is_empty() {
            self.set_selected_row(0, scroll);
            return;
        }

        let primary_row = self
            .ivars()
            .selected_row
            .borrow()
            .as_ref()
            .copied()
            .filter(|row| selected_rows.contains(row))
            .or_else(|| selected_rows.first().copied());
        self.set_selected_rows(selected_rows.as_slice(), primary_row, scroll);
    }

    pub(in crate::platform::macos) fn move_selection(&self, delta: NSInteger) {
        let row_count = self.ivars().filtered_rows.borrow().len() as NSInteger;
        if row_count == 0 {
            return;
        }

        let current = self
            .ivars()
            .selected_row
            .borrow()
            .as_ref()
            .copied()
            .map(|row| row as NSInteger)
            .unwrap_or(0);
        let next = if current < 0 {
            0
        } else {
            (current + delta).clamp(0, row_count - 1)
        };
        self.set_selected_row(next as usize, true);
    }

    pub(in crate::platform::macos) fn sync_selected_row_from_table(&self) {
        let Some(table_view) = self.table_view() else {
            return;
        };
        let rows = selected_rows_from_table(&table_view);
        let primary_row = {
            let row = table_view.selectedRow();
            (row >= 0)
                .then_some(row as usize)
                .or_else(|| rows.last().copied())
        };
        let (sanitized_rows, sanitized_primary_row) = {
            let filtered_rows = self.ivars().filtered_rows.borrow();
            sanitize_multi_selection_rows(rows.as_slice(), primary_row, filtered_rows.as_slice())
        };
        if sanitized_rows != rows || sanitized_primary_row != primary_row {
            if sanitized_rows.is_empty() {
                self.clear_history_selection();
            } else {
                self.set_selected_rows(sanitized_rows.as_slice(), sanitized_primary_row, false);
            }
            return;
        }

        let selected_ids = self.history_ids_for_rows(sanitized_rows.as_slice());
        self.ivars().selected_history_ids.replace(selected_ids);
        self.ivars().selected_row.replace(sanitized_primary_row);
    }

    pub(in crate::platform::macos) fn context_history_row(&self) -> Option<usize> {
        let table_view = self.table_view()?;
        let row = table_view.clickedRow();
        (row >= 0).then_some(row as usize)
    }

    pub(in crate::platform::macos) fn refresh_history_context_menu_state(&self) {
        let row = self
            .context_history_row()
            .or_else(|| self.ivars().selected_row.borrow().as_ref().copied());

        if let Some(row) = row {
            let selected_rows = self.current_selected_history_rows();
            if !selected_rows.contains(&row) {
                self.set_selected_row(row, false);
            }
        }

        let selected_items = self
            .current_selected_history_ids()
            .into_iter()
            .filter_map(|id| {
                self.ivars()
                    .controller
                    .borrow()
                    .load_item(id)
                    .ok()
                    .flatten()
            })
            .collect::<Vec<_>>();
        let local_items = selected_items
            .iter()
            .filter(|item| !item.is_remote)
            .collect::<Vec<_>>();
        let can_edit_local_item = !local_items.is_empty();
        let can_open_folder = local_items.iter().any(|item| {
            matches!(&item.payload, ClipboardPayload::Files(files) if !files.is_empty())
                && !item.local_file_parent_directories().is_empty()
        });
        let is_pinned = !local_items.is_empty() && local_items.iter().all(|item| item.is_pinned);
        let row_tag = row.map(|row| row as NSInteger).unwrap_or(-1);

        if let Some(item) = self.history_context_pin_item() {
            item.setTag(row_tag);
            item.setTitle(&NSString::from_str(if is_pinned {
                "取消锁定"
            } else {
                "锁定"
            }));
            item.setEnabled(can_edit_local_item);
            item.setHidden(!can_edit_local_item);
        }
        if let Some(item) = self.history_context_open_folder_item() {
            item.setTag(row_tag);
            item.setEnabled(can_open_folder);
            item.setHidden(!can_open_folder);
        }
        if let Some(item) = self.history_context_separator_item() {
            item.setHidden(!can_edit_local_item);
        }
        if let Some(item) = self.history_context_delete_item() {
            item.setTag(row_tag);
            item.setEnabled(can_edit_local_item);
            item.setHidden(!can_edit_local_item);
        }
    }

    pub(in crate::platform::macos) fn set_selected_row(&self, row: usize, scroll: bool) {
        self.set_selected_rows(&[row], Some(row), scroll);
    }

    pub(in crate::platform::macos) fn set_selected_rows(
        &self,
        rows: &[usize],
        primary_row: Option<usize>,
        scroll: bool,
    ) {
        let Some(table_view) = self.table_view() else {
            return;
        };
        let indexes = index_set_from_rows(rows);
        let selected_ids = self.history_ids_for_rows(rows);
        self.ivars().selected_row.replace(primary_row);
        self.ivars().selected_history_ids.replace(selected_ids);
        table_view.selectRowIndexes_byExtendingSelection(&indexes, false);
        if scroll {
            if let Some(row) = primary_row {
                table_view.scrollRowToVisible(row as NSInteger);
            } else if let Some(row) = rows.first().copied() {
                table_view.scrollRowToVisible(row as NSInteger);
            }
        }
    }

    fn current_selected_history_id(&self) -> Option<Uuid> {
        let selected_row = self.ivars().selected_row.borrow().as_ref().copied()?;
        self.ivars()
            .filtered_rows
            .borrow()
            .get(selected_row)
            .map(|row| row.id)
    }

    pub(in crate::platform::macos) fn current_selected_history_ids(&self) -> Vec<Uuid> {
        let selected_ids = self.ivars().selected_history_ids.borrow().clone();
        if !selected_ids.is_empty() {
            return selected_ids;
        }

        self.current_selected_history_id()
            .map(|id| vec![id])
            .unwrap_or_default()
    }

    pub(in crate::platform::macos) fn current_selected_history_rows(&self) -> Vec<usize> {
        let selected_ids = self.current_selected_history_ids();
        let rows = self.ivars().filtered_rows.borrow();
        selected_ids
            .into_iter()
            .filter_map(|id| rows.iter().position(|row| row.id == id))
            .collect()
    }

    pub(in crate::platform::macos) fn local_rows_for_batch_actions(
        &self,
        rows: &[usize],
    ) -> Vec<usize> {
        let filtered_rows = self.ivars().filtered_rows.borrow();
        local_rows_for_batch_actions(rows, filtered_rows.as_slice())
    }

    pub(in crate::platform::macos) fn clear_history_selection(&self) {
        self.ivars().selected_row.replace(None);
        self.ivars().selected_history_ids.replace(Vec::new());
        if let Some(table_view) = self.table_view() {
            unsafe {
                table_view.deselectAll(None);
            }
        }
    }

    fn current_scroll_origin(&self) -> Option<NSPoint> {
        let table_view = self.table_view()?;
        let scroll_view = table_view.enclosingScrollView()?;
        Some(scroll_view.documentVisibleRect().origin)
    }

    fn restore_scroll_origin(&self, origin: Option<NSPoint>) {
        let Some(origin) = origin else {
            return;
        };
        let Some(table_view) = self.table_view() else {
            return;
        };
        table_view.scrollPoint(NSPoint::new(0.0, origin.y.max(0.0)));
    }
    fn history_ids_for_rows(&self, rows: &[usize]) -> Vec<Uuid> {
        let filtered_rows = self.ivars().filtered_rows.borrow();
        rows.iter()
            .filter_map(|row| filtered_rows.get(*row).map(|entry| entry.id))
            .collect()
    }
}

fn sanitize_multi_selection_rows(
    rows: &[usize],
    primary_row: Option<usize>,
    filtered_rows: &[HistoryRow],
) -> (Vec<usize>, Option<usize>) {
    if rows.len() <= 1 {
        return (rows.to_vec(), primary_row);
    }

    let local_rows = local_rows_for_batch_actions(rows, filtered_rows);
    if !local_rows.is_empty() {
        let primary_row = primary_row
            .filter(|row| local_rows.contains(row))
            .or_else(|| local_rows.last().copied());
        return (local_rows, primary_row);
    }

    let fallback_row = primary_row
        .filter(|row| filtered_rows.get(*row).is_some())
        .or_else(|| rows.last().copied());
    match fallback_row {
        Some(row) => (vec![row], Some(row)),
        None => (Vec::new(), None),
    }
}

fn local_rows_for_batch_actions(rows: &[usize], filtered_rows: &[HistoryRow]) -> Vec<usize> {
    if rows.len() <= 1 {
        return rows.to_vec();
    }

    let mut local_rows = rows
        .iter()
        .copied()
        .filter(|row| {
            filtered_rows
                .get(*row)
                .is_some_and(|entry| !entry.is_remote)
        })
        .collect::<Vec<_>>();
    local_rows.sort_unstable();
    local_rows.dedup();
    local_rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::HistoryRow;
    use uuid::Uuid;

    fn row(is_remote: bool) -> HistoryRow {
        HistoryRow {
            id: Uuid::new_v4(),
            kind: "text".to_string(),
            source_badge: if is_remote {
                "远端".to_string()
            } else {
                "本机".to_string()
            },
            source_tooltip: String::new(),
            summary_text: String::new(),
            detail_tooltip: String::new(),
            is_remote,
            is_pinned: false,
        }
    }

    #[test]
    fn sanitize_multi_selection_prefers_local_rows_for_batch_state() {
        let rows = vec![row(false), row(true), row(false)];
        let (sanitized_rows, primary_row) =
            sanitize_multi_selection_rows(&[0, 1, 2], Some(1), rows.as_slice());

        assert_eq!(sanitized_rows, vec![0, 2]);
        assert_eq!(primary_row, Some(2));
    }

    #[test]
    fn sanitize_multi_selection_falls_back_to_single_remote_row() {
        let rows = vec![row(true), row(true)];
        let (sanitized_rows, primary_row) =
            sanitize_multi_selection_rows(&[0, 1], Some(1), rows.as_slice());

        assert_eq!(sanitized_rows, vec![1]);
        assert_eq!(primary_row, Some(1));
    }
}
