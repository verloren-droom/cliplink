use super::{
    ui::{FOOTER_HEIGHT, HEADER_HEIGHT, OUTER_PADDING, PANEL_HEIGHT, PANEL_WIDTH, ROW_HEIGHT},
    *,
};
use objc2::sel;
use objc2_foundation::ns_string;

use crate::constants::app::APP_NAME;
use crate::platform::macos::widgets::{FooterShortcut, make_footer_button, make_separator};

const FOOTER_BUTTON_HEIGHT: f64 = 22.0;
const FOOTER_BUTTON_X_OFFSET: f64 = 6.0;
const FOOTER_BUTTON_CLEAR_Y: f64 = 70.0;
const FOOTER_BUTTON_PREFERENCES_Y: f64 = 48.0;
const FOOTER_BUTTON_ABOUT_Y: f64 = 26.0;
const FOOTER_BUTTON_QUIT_Y: f64 = 4.0;
const FOOTER_BUTTON_HORIZONTAL_PADDING: f64 = 4.0;

impl AppDelegate {
    pub(super) fn install_panel(&self, mtm: MainThreadMarker) {
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::FullSizeContentView
            | NSWindowStyleMask::NonactivatingPanel;

        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(PANEL_WIDTH, PANEL_HEIGHT),
            ),
            style,
            NSBackingStoreType::Buffered,
            false,
        );

        unsafe { panel.setReleasedWhenClosed(false) };
        panel.setFloatingPanel(true);
        panel.setBecomesKeyOnlyIfNeeded(false);
        panel.setWorksWhenModal(true);
        panel.setTitle(&NSString::from_str(APP_NAME));
        panel.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        panel.setTitlebarAppearsTransparent(true);
        panel.setMovableByWindowBackground(false);
        panel.setOpaque(false);
        panel.setHasShadow(true);
        panel.setHidesOnDeactivate(true);
        panel.setLevel(NSPopUpMenuWindowLevel.max(NSFloatingWindowLevel));
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::MoveToActiveSpace
                | NSWindowCollectionBehavior::Transient
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        panel.setBackgroundColor(Some(&NSColor::windowBackgroundColor()));
        panel.setDelegate(Some(ProtocolObject::from_ref(self)));

        if let Some(button) = panel.standardWindowButton(objc2_app_kit::NSWindowButton::CloseButton)
        {
            button.setHidden(true);
        }
        if let Some(button) =
            panel.standardWindowButton(objc2_app_kit::NSWindowButton::MiniaturizeButton)
        {
            button.setHidden(true);
        }
        if let Some(button) = panel.standardWindowButton(objc2_app_kit::NSWindowButton::ZoomButton)
        {
            button.setHidden(true);
        }

        let Some(content) = panel.contentView() else {
            return;
        };

        let search_frame = NSRect::new(
            NSPoint::new(OUTER_PADDING, PANEL_HEIGHT - HEADER_HEIGHT + 4.0),
            NSSize::new(PANEL_WIDTH - OUTER_PADDING * 2.0, 30.0),
        );
        let search_field = NSSearchField::initWithFrame(NSSearchField::alloc(mtm), search_frame);
        search_field.setPlaceholderString(Some(ns_string!("搜索")));
        unsafe { search_field.setDelegate(Some(ProtocolObject::from_ref(self))) };

        let table_height = PANEL_HEIGHT - HEADER_HEIGHT - FOOTER_HEIGHT - OUTER_PADDING;
        let scroll_frame = NSRect::new(
            NSPoint::new(OUTER_PADDING, FOOTER_HEIGHT),
            NSSize::new(PANEL_WIDTH - OUTER_PADDING * 2.0, table_height),
        );
        let scroll_view = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), scroll_frame);
        scroll_view.setDrawsBackground(false);
        scroll_view.setBorderType(objc2_app_kit::NSBorderType::NoBorder);
        scroll_view.setHasVerticalScroller(true);

        let table_view = NSTableView::initWithFrame(
            NSTableView::alloc(mtm),
            NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(scroll_frame.size.width, table_height),
            ),
        );
        table_view.setHeaderView(None);
        table_view.setUsesAlternatingRowBackgroundColors(false);
        table_view.setBackgroundColor(&NSColor::clearColor());
        table_view.setColumnAutoresizingStyle(
            NSTableViewColumnAutoresizingStyle::FirstColumnOnlyAutoresizingStyle,
        );
        table_view.setSelectionHighlightStyle(NSTableViewSelectionHighlightStyle::Regular);
        table_view.setStyle(NSTableViewStyle::Inset);
        table_view.setRowSizeStyle(NSTableViewRowSizeStyle::Small);
        table_view.setRowHeight(ROW_HEIGHT);
        table_view.setIntercellSpacing(NSSize::new(0.0, 0.0));
        table_view.setUsesAutomaticRowHeights(false);
        unsafe {
            table_view.setDataSource(Some(ProtocolObject::from_ref(self)));
            table_view.setDelegate(Some(ProtocolObject::from_ref(self)));
            table_view.setTarget(Some(self));
            table_view.setAction(Some(sel!(activateSelected:)));
            table_view.setDoubleAction(Some(sel!(activateSelected:)));
        }

        let history_context_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("历史"));
        let pin_item = crate::platform::macos::widgets::make_menu_item(
            mtm,
            "锁定/取消锁定",
            Some(self),
            Some(sel!(toggleHistoryItemPin:)),
            "",
        );
        pin_item.setTag(-1);
        history_context_menu.addItem(&pin_item);
        history_context_menu.addItem(&NSMenuItem::separatorItem(mtm));
        let delete_item = crate::platform::macos::widgets::make_menu_item(
            mtm,
            "删除",
            Some(self),
            Some(sel!(deleteHistoryItem:)),
            "",
        );
        delete_item.setTag(-1);
        history_context_menu.addItem(&delete_item);
        unsafe {
            table_view.setMenu(Some(&history_context_menu));
        }

        let column =
            NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), ns_string!("main"));
        column.setEditable(false);
        column.setResizingMask(NSTableColumnResizingOptions::AutoresizingMask);
        column.setWidth(scroll_frame.size.width);
        table_view.addTableColumn(&column);
        scroll_view.setDocumentView(Some(&table_view));

        let separator = make_separator(
            mtm,
            NSRect::new(
                NSPoint::new(OUTER_PADDING, FOOTER_HEIGHT - 4.0),
                NSSize::new(PANEL_WIDTH - OUTER_PADDING * 2.0, 1.0),
            ),
        );

        let clear_button = make_footer_button(
            mtm,
            "清除",
            self,
            sel!(clearHistory:),
            NSRect::new(
                NSPoint::new(
                    OUTER_PADDING + FOOTER_BUTTON_X_OFFSET,
                    FOOTER_BUTTON_CLEAR_Y,
                ),
                NSSize::new(
                    PANEL_WIDTH - OUTER_PADDING * 2.0 - FOOTER_BUTTON_HORIZONTAL_PADDING * 2.0,
                    FOOTER_BUTTON_HEIGHT,
                ),
            ),
            None,
        );

        let preferences_button = make_footer_button(
            mtm,
            "偏好设置...",
            self,
            sel!(openPreferences:),
            NSRect::new(
                NSPoint::new(
                    OUTER_PADDING + FOOTER_BUTTON_X_OFFSET,
                    FOOTER_BUTTON_PREFERENCES_Y,
                ),
                NSSize::new(
                    PANEL_WIDTH - OUTER_PADDING * 2.0 - FOOTER_BUTTON_HORIZONTAL_PADDING * 2.0,
                    FOOTER_BUTTON_HEIGHT,
                ),
            ),
            Some(FooterShortcut {
                display: "⌘,",
                key_equivalent: ",",
                modifier_mask: NSEventModifierFlags::Command,
            }),
        );

        let about_button = make_footer_button(
            mtm,
            "关于",
            self,
            sel!(openAbout:),
            NSRect::new(
                NSPoint::new(
                    OUTER_PADDING + FOOTER_BUTTON_X_OFFSET,
                    FOOTER_BUTTON_ABOUT_Y,
                ),
                NSSize::new(
                    PANEL_WIDTH - OUTER_PADDING * 2.0 - FOOTER_BUTTON_HORIZONTAL_PADDING * 2.0,
                    FOOTER_BUTTON_HEIGHT,
                ),
            ),
            None,
        );

        let quit_button = make_footer_button(
            mtm,
            "退出",
            self,
            sel!(quitApp:),
            NSRect::new(
                NSPoint::new(OUTER_PADDING + FOOTER_BUTTON_X_OFFSET, FOOTER_BUTTON_QUIT_Y),
                NSSize::new(
                    PANEL_WIDTH - OUTER_PADDING * 2.0 - FOOTER_BUTTON_HORIZONTAL_PADDING * 2.0,
                    FOOTER_BUTTON_HEIGHT,
                ),
            ),
            Some(FooterShortcut {
                display: "⌘Q",
                key_equivalent: "q",
                modifier_mask: NSEventModifierFlags::Command,
            }),
        );

        content.addSubview(&search_field);
        content.addSubview(&scroll_view);
        content.addSubview(&separator);
        content.addSubview(&clear_button);
        content.addSubview(&preferences_button);
        content.addSubview(&about_button);
        content.addSubview(&quit_button);

        panel.setInitialFirstResponder(Some(&search_field));

        self.ivars().panel.replace(Some(panel));
        self.ivars().search_field.replace(Some(search_field));
        self.ivars().table_view.replace(Some(table_view));
        self.ivars()
            .history_context_menu
            .replace(Some(history_context_menu));
    }

    pub(super) fn ensure_panel(&self) {
        if self.panel().is_none() {
            self.install_panel(self.mtm());
            self.reload_filtered_rows_revealing_selection();
        }
    }

    pub(super) fn toggle_panel(&self) {
        self.ensure_panel();

        let Some(panel) = self.panel() else {
            return;
        };

        if panel.isVisible() {
            self.hide_panel(true);
        } else {
            self.show_panel();
        }
    }

    pub(super) fn show_panel(&self) {
        self.ensure_panel();
        self.capture_frontmost_application();

        let mtm = self.mtm();
        let app = NSApplication::sharedApplication(mtm);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);

        self.position_panel();
        self.reload_filtered_rows_revealing_selection();

        let Some(panel) = self.panel() else {
            return;
        };
        panel.makeKeyAndOrderFront(None);

        if let Some(search_field) = self.search_field() {
            let _ = panel.makeFirstResponder(Some(search_field.as_ref()));
        }
    }

    pub(super) fn hide_panel(&self, clear_search: bool) {
        let Some(panel) = self.panel() else {
            return;
        };

        panel.orderOut(None);

        if clear_search {
            self.ivars().search_query.replace(String::new());
            if let Some(search_field) = self.search_field() {
                search_field.setStringValue(ns_string!(""));
            }
        }

        self.release_panel();
    }

    fn position_panel(&self) {
        let Some(panel) = self.panel() else {
            return;
        };
        let mtm = self.mtm();
        let mouse = NSEvent::mouseLocation();
        let visible_frame = NSScreen::screens(mtm)
            .iter()
            .find_map(|screen| {
                let frame = screen.visibleFrame();
                if point_in_rect(mouse, frame) {
                    Some(frame)
                } else {
                    None
                }
            })
            .or_else(|| NSScreen::mainScreen(mtm).map(|screen| screen.visibleFrame()))
            .unwrap_or_else(|| {
                NSRect::new(
                    NSPoint::new(0.0, 0.0),
                    NSSize::new(PANEL_WIDTH + 32.0, PANEL_HEIGHT + 32.0),
                )
            });

        let margin = 12.0;
        let min_x = visible_frame.origin.x + margin;
        let max_x =
            (visible_frame.origin.x + visible_frame.size.width - PANEL_WIDTH - margin).max(min_x);
        let ideal_x = mouse.x - PANEL_WIDTH / 2.0;

        let min_top = visible_frame.origin.y + HEADER_HEIGHT + 18.0;
        let max_top = visible_frame.origin.y + visible_frame.size.height - margin;
        let ideal_top = mouse.y + HEADER_HEIGHT + 14.0;

        panel.setFrameTopLeftPoint(NSPoint::new(
            ideal_x.clamp(min_x, max_x),
            ideal_top.clamp(min_top, max_top),
        ));
    }

    pub(super) fn reload_filtered_rows(&self) {
        self.reload_filtered_rows_with_options(false);
    }

    pub(super) fn reload_filtered_rows_revealing_selection(&self) {
        self.reload_filtered_rows_with_options(true);
    }

    fn reload_filtered_rows_with_options(&self, reveal_selection: bool) {
        let selected_id = self.current_selected_history_id();
        let previous_scroll_origin = self.current_scroll_origin();
        let query = self.ivars().search_query.borrow().clone();
        let rows = self.ivars().controller.borrow().history_rows(&query);
        let matched_selection = selected_id.and_then(|id| rows.iter().position(|row| row.id == id));
        *self.ivars().filtered_rows.borrow_mut() = rows;
        if let Some(row) = matched_selection {
            self.ivars().selected_row.replace(Some(row));
        }

        let Some(table_view) = self.table_view() else {
            return;
        };

        table_view.reloadData();
        let selected_row = self.ivars().selected_row.borrow().as_ref().copied();
        let row_count = self.ivars().filtered_rows.borrow().len();
        let should_reveal_selection = reveal_selection
            || selected_row.is_none()
            || selected_row.is_some_and(|row| row >= row_count);
        self.ensure_selection(should_reveal_selection);
        if !should_reveal_selection {
            self.restore_scroll_origin(previous_scroll_origin);
        }
    }

    fn ensure_selection(&self, scroll: bool) {
        let Some(table_view) = self.table_view() else {
            return;
        };
        let rows = self.ivars().filtered_rows.borrow().len();
        if rows == 0 {
            self.ivars().selected_row.replace(None);
            unsafe {
                table_view.deselectAll(None);
            }
            return;
        }

        let target = self
            .ivars()
            .selected_row
            .borrow()
            .as_ref()
            .copied()
            .filter(|row| *row < rows)
            .unwrap_or(0);
        self.set_selected_row(target, scroll);
    }

    pub(super) fn move_selection(&self, delta: NSInteger) {
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

    pub(super) fn activate_selected(&self) {
        let Some(row) = self.ivars().selected_row.borrow().as_ref().copied() else {
            return;
        };

        self.activate_row(row);
    }

    pub(super) fn activate_clicked_or_selected(&self, sender: Option<&AnyObject>) {
        let clicked_row = sender
            .map(|sender| unsafe { msg_send![sender, clickedRow] })
            .filter(|row: &NSInteger| *row >= 0)
            .map(|row| row as usize);

        let row = clicked_row.or_else(|| self.ivars().selected_row.borrow().as_ref().copied());
        let Some(row) = row else {
            return;
        };

        self.set_selected_row(row, sender.is_none());
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

        self.hide_panel(true);
        if self.ivars().controller.borrow_mut().copy_item(id).ok() == Some(true) {
            self.trigger_immediate_paste();
        }
    }

    pub(super) fn delete_history_row(&self, row: usize) {
        let Some(id) = self
            .ivars()
            .filtered_rows
            .borrow()
            .get(row)
            .map(|entry| entry.id)
        else {
            return;
        };

        if self
            .ivars()
            .controller
            .borrow_mut()
            .delete_history_item(id)
            .ok()
            != Some(true)
        {
            return;
        }

        self.reload_filtered_rows();

        let remaining = self.ivars().filtered_rows.borrow().len();
        if remaining == 0 {
            self.ivars().selected_row.replace(None);
            if let Some(table_view) = self.table_view() {
                unsafe {
                    table_view.deselectAll(None);
                }
            }
            return;
        }

        let target = row.saturating_sub(1).min(remaining - 1);
        self.set_selected_row(target, true);
    }

    pub(super) fn toggle_history_row_pin(&self, row: usize) {
        let Some(id) = self
            .ivars()
            .filtered_rows
            .borrow()
            .get(row)
            .map(|entry| entry.id)
        else {
            return;
        };

        if self
            .ivars()
            .controller
            .borrow_mut()
            .toggle_history_item_pin(id)
            .ok()
            .flatten()
            .is_none()
        {
            return;
        }

        self.reload_filtered_rows_revealing_selection();
    }

    pub(super) fn sync_selected_row_from_table(&self) {
        let Some(table_view) = self.table_view() else {
            return;
        };
        let row = table_view.selectedRow();
        self.ivars()
            .selected_row
            .replace((row >= 0).then_some(row as usize));
    }

    pub(super) fn context_history_row(&self) -> Option<usize> {
        let table_view = self.table_view()?;
        let row = table_view.clickedRow();
        (row >= 0).then_some(row as usize)
    }

    fn set_selected_row(&self, row: usize, scroll: bool) {
        let Some(table_view) = self.table_view() else {
            return;
        };

        self.ivars().selected_row.replace(Some(row));
        let indexes = NSIndexSet::indexSetWithIndex(row as NSUInteger);
        table_view.selectRowIndexes_byExtendingSelection(&indexes, false);
        if scroll {
            table_view.scrollRowToVisible(row as NSInteger);
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

    fn release_panel(&self) {
        if let Some(table_view) = self.ivars().table_view.borrow_mut().take() {
            unsafe {
                table_view.setDelegate(None);
                table_view.setDataSource(None);
                table_view.setTarget(None);
                table_view.setMenu(None);
            }
        }
        if let Some(search_field) = self.ivars().search_field.borrow_mut().take() {
            unsafe {
                search_field.setDelegate(None);
            }
        }
        self.ivars().history_context_menu.borrow_mut().take();

        if let Some(panel) = self.ivars().panel.borrow_mut().take() {
            panel.setDelegate(None);
            panel.orderOut(None);
            panel.setContentView(None);
            panel.close();
        }

        self.ivars().filtered_rows.borrow_mut().clear();
        self.ivars().selected_row.replace(None);
    }
}

fn point_in_rect(point: NSPoint, rect: NSRect) -> bool {
    point.x >= rect.origin.x
        && point.x <= rect.origin.x + rect.size.width
        && point.y >= rect.origin.y
        && point.y <= rect.origin.y + rect.size.height
}
