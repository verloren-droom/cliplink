use super::*;

impl AppDelegate {
    pub(in crate::platform::macos) fn confirm_clear_history(&self) -> Option<bool> {
        let alert = NSAlert::new(self.mtm());
        alert.setMessageText(&NSString::from_str("确认清除历史记录"));
        alert.setInformativeText(&NSString::from_str(
            "默认仅清除本机未锁定的剪切板历史记录。\n\n远程历史始终保留。",
        ));
        let accessory = NSView::initWithFrame(
            NSView::alloc(self.mtm()),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(260.0, 22.0)),
        );
        let include_pinned_checkbox = make_checkbox(
            self.mtm(),
            "同时清除锁定条目",
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(260.0, 22.0)),
        );
        include_pinned_checkbox.setState(objc2_app_kit::NSControlStateValueOff);
        include_pinned_checkbox.sizeToFit();
        let mut checkbox_frame = include_pinned_checkbox.frame();
        checkbox_frame.origin.x = ((260.0 - checkbox_frame.size.width) / 2.0).max(0.0);
        checkbox_frame.origin.y = ((22.0 - checkbox_frame.size.height) / 2.0).max(0.0);
        include_pinned_checkbox.setFrame(checkbox_frame);
        accessory.addSubview(&include_pinned_checkbox);
        alert.setAccessoryView(Some(&accessory));
        alert.addButtonWithTitle(&NSString::from_str("清除"));
        alert.addButtonWithTitle(&NSString::from_str("取消"));
        if alert.runModal() != NSAlertFirstButtonReturn {
            return None;
        }

        Some(include_pinned_checkbox.state() == objc2_app_kit::NSControlStateValueOn)
    }

    pub(in crate::platform::macos) fn install_panel(&self, mtm: MainThreadMarker) {
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

        let scope_width = 132.0;
        let scope_frame = NSRect::new(
            NSPoint::new(OUTER_PADDING, PANEL_HEIGHT - HEADER_HEIGHT + 4.0),
            NSSize::new(scope_width, 30.0),
        );
        let history_scope_button =
            NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), scope_frame, false);
        unsafe {
            history_scope_button.setTarget(Some(self));
            history_scope_button.setAction(Some(sel!(historyScopeChanged:)));
        }

        let search_frame = NSRect::new(
            NSPoint::new(
                OUTER_PADDING + scope_width + 8.0,
                PANEL_HEIGHT - HEADER_HEIGHT + 4.0,
            ),
            NSSize::new(PANEL_WIDTH - OUTER_PADDING * 2.0 - scope_width - 8.0, 30.0),
        );
        let search_field = NSSearchField::initWithFrame(NSSearchField::alloc(mtm), search_frame);
        search_field.setPlaceholderString(Some(ns_string!("搜索")));
        unsafe { search_field.setDelegate(Some(ProtocolObject::from_ref(self))) };

        let table_height =
            PANEL_HEIGHT - HEADER_HEIGHT - FOOTER_HEIGHT - OUTER_PADDING - FEEDBACK_HEIGHT;
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
        table_view.setAllowsMultipleSelection(true);
        unsafe {
            table_view.setDataSource(Some(ProtocolObject::from_ref(self)));
            table_view.setDelegate(Some(ProtocolObject::from_ref(self)));
            table_view.setTarget(Some(self));
            table_view.setAction(Some(sel!(activateSelected:)));
            table_view.setDoubleAction(Some(sel!(activateSelected:)));
        }

        let history_context_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("历史"));
        history_context_menu.setDelegate(Some(ProtocolObject::from_ref(self)));
        let pin_item = crate::platform::macos::widgets::make_menu_item(
            mtm,
            "锁定",
            Some(self),
            Some(sel!(toggleHistoryItemPin:)),
            "",
        );
        pin_item.setTag(-1);
        history_context_menu.addItem(&pin_item);
        let open_folder_item = crate::platform::macos::widgets::make_menu_item(
            mtm,
            "打开所在文件夹",
            Some(self),
            Some(sel!(openHistoryItemParentFolders:)),
            "",
        );
        open_folder_item.setTag(-1);
        history_context_menu.addItem(&open_folder_item);
        let separator_item = NSMenuItem::separatorItem(mtm);
        history_context_menu.addItem(&separator_item);
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

        let feedback_track = make_background_box(
            mtm,
            NSRect::new(
                NSPoint::new(OUTER_PADDING, FOOTER_HEIGHT + table_height + 1.0),
                NSSize::new(PANEL_WIDTH - OUTER_PADDING * 2.0, FEEDBACK_BAR_HEIGHT),
            ),
        );
        feedback_track.setHidden(true);
        feedback_track.setBackgroundColor(Some(
            &NSColor::secondaryLabelColor().colorWithAlphaComponent(0.12),
        ));
        let feedback_fill = make_background_box(
            mtm,
            NSRect::new(
                NSPoint::new(OUTER_PADDING, FOOTER_HEIGHT + table_height + 1.0),
                NSSize::new(0.0, FEEDBACK_BAR_HEIGHT),
            ),
        );
        feedback_fill.setHidden(true);
        feedback_fill.setBackgroundColor(Some(&NSColor::systemBlueColor()));

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
            Some(FooterShortcut {
                display: "⌘⌫",
                key_equivalent: "\u{8}",
                modifier_mask: NSEventModifierFlags::Command,
            }),
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

        content.addSubview(&history_scope_button);
        content.addSubview(&search_field);
        content.addSubview(&feedback_track);
        content.addSubview(&feedback_fill);
        content.addSubview(&scroll_view);
        content.addSubview(&separator);
        content.addSubview(&clear_button);
        content.addSubview(&preferences_button);
        content.addSubview(&about_button);
        content.addSubview(&quit_button);

        panel.setInitialFirstResponder(Some(&search_field));

        self.ivars().panel.replace(Some(panel));
        self.ivars().search_field.replace(Some(search_field));
        self.ivars()
            .history_scope_button
            .replace(Some(history_scope_button));
        self.ivars().table_view.replace(Some(table_view));
        self.ivars()
            .panel_progress_track
            .replace(Some(feedback_track));
        self.ivars()
            .panel_progress_fill
            .replace(Some(feedback_fill));
        self.ivars()
            .history_context_separator_item
            .replace(Some(separator_item));
        self.ivars()
            .history_context_pin_item
            .replace(Some(pin_item));
        self.ivars()
            .history_context_open_folder_item
            .replace(Some(open_folder_item));
        self.ivars()
            .history_context_delete_item
            .replace(Some(delete_item));
        self.ivars()
            .history_context_menu
            .replace(Some(history_context_menu));
    }

    pub(in crate::platform::macos) fn ensure_panel(&self) {
        if self.panel().is_none() {
            self.install_panel(self.mtm());
            self.reload_filtered_rows_revealing_selection();
        }
    }

    pub(in crate::platform::macos) fn toggle_panel(&self) {
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

    pub(in crate::platform::macos) fn show_panel(&self) {
        self.ensure_panel();
        self.capture_frontmost_application();

        let mtm = self.mtm();
        let app = NSApplication::sharedApplication(mtm);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);

        self.position_panel();
        self.reload_filtered_rows_revealing_selection();
        self.refresh_panel_feedback_state();

        let Some(panel) = self.panel() else {
            return;
        };
        panel.makeKeyAndOrderFront(None);

        if let Some(search_field) = self.search_field() {
            let _ = panel.makeFirstResponder(Some(search_field.as_ref()));
        }
    }

    pub(in crate::platform::macos) fn hide_panel(&self, clear_search: bool) {
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
        if let Some(menu) = self.ivars().history_context_menu.borrow_mut().take() {
            menu.setDelegate(None);
        }
        self.ivars().history_context_separator_item.replace(None);
        self.ivars().history_context_pin_item.replace(None);
        self.ivars().history_context_open_folder_item.replace(None);
        self.ivars().history_context_delete_item.replace(None);
        self.ivars().panel_status_label.replace(None);
        self.ivars().panel_progress_track.replace(None);
        self.ivars().panel_progress_fill.replace(None);

        if let Some(panel) = self.ivars().panel.borrow_mut().take() {
            panel.setDelegate(None);
            panel.orderOut(None);
            panel.setContentView(None);
            panel.close();
        }

        let mut filtered_rows = self.ivars().filtered_rows.borrow_mut();
        filtered_rows.clear();
        filtered_rows.shrink_to_fit();
        drop(filtered_rows);
        self.ivars().selected_row.replace(None);
        self.ivars().selected_history_ids.replace(Vec::new());
    }
}
