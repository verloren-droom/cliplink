use super::*;

impl AppDelegate {
    pub(in crate::platform::macos) fn install_preferences_window(&self, mtm: MainThreadMarker) {
        let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable;
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                NSRect::new(
                    NSPoint::new(0.0, 0.0),
                    NSSize::new(PREFERENCES_WIDTH, PREFERENCES_HEIGHT),
                ),
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(ns_string!("偏好设置"));
        window.setTitlebarAppearsTransparent(false);
        window.center();
        window.setDelegate(Some(ProtocolObject::from_ref(self)));

        let Some(content) = window.contentView() else {
            return;
        };

        let tab_view = NSTabView::initWithFrame(
            NSTabView::alloc(mtm),
            NSRect::new(
                NSPoint::new(12.0, 52.0),
                NSSize::new(PREFERENCES_WIDTH - 24.0, PREFERENCES_HEIGHT - 72.0),
            ),
        );
        tab_view.setTabViewType(NSTabViewType::TopTabsBezelBorder);

        let tab_size = tab_view.contentRect().size;

        let (general_view, device_name_field, history_limit_field, launch_at_login_checkbox) =
            self.build_general_preferences_tab(mtm, tab_size);
        let (hotkey_view, hotkey_field, hotkey_record_button, hotkey_preview_label) =
            self.build_hotkey_preferences_tab(mtm, tab_size);
        let (sharing_view, share_checkbox, prefer_remote_paste_checkbox, discovery_checkbox) =
            self.build_sharing_preferences_tab(mtm, tab_size);
        let (
            devices_view,
            devices_table,
            devices_context_menu,
            devices_context_action_item,
            devices_context_properties_item,
        ) = self.build_devices_preferences_tab(mtm, tab_size);

        let general_tab = NSTabViewItem::new();
        general_tab.setLabel(ns_string!("通用"));
        general_tab.setView(Some(&general_view));

        let hotkey_tab = NSTabViewItem::new();
        hotkey_tab.setLabel(ns_string!("快捷键"));
        hotkey_tab.setView(Some(&hotkey_view));

        let sharing_tab = NSTabViewItem::new();
        sharing_tab.setLabel(ns_string!("共享"));
        sharing_tab.setView(Some(&sharing_view));

        let devices_tab = NSTabViewItem::new();
        devices_tab.setLabel(ns_string!("设备"));
        devices_tab.setView(Some(&devices_view));

        tab_view.addTabViewItem(&general_tab);
        tab_view.addTabViewItem(&hotkey_tab);
        tab_view.addTabViewItem(&sharing_tab);
        tab_view.addTabViewItem(&devices_tab);
        tab_view.selectTabViewItemAtIndex(0);

        let status_label = make_secondary_label(
            mtm,
            "",
            NSRect::new(NSPoint::new(20.0, 18.0), NSSize::new(300.0, 20.0)),
        );

        let cancel_button = unsafe {
            NSButton::buttonWithTitle_target_action(
                ns_string!("取消"),
                Some(self),
                Some(sel!(closePreferences:)),
                mtm,
            )
        };
        cancel_button.sizeToFit();
        cancel_button.setFrameOrigin(NSPoint::new(PREFERENCES_WIDTH - 160.0, 14.0));

        let save_button = unsafe {
            NSButton::buttonWithTitle_target_action(
                ns_string!("保存"),
                Some(self),
                Some(sel!(savePreferences:)),
                mtm,
            )
        };
        save_button.sizeToFit();
        save_button.setFrameOrigin(NSPoint::new(PREFERENCES_WIDTH - 88.0, 14.0));
        save_button.setEnabled(false);

        content.addSubview(&tab_view);
        content.addSubview(&status_label);
        content.addSubview(&cancel_button);
        content.addSubview(&save_button);

        self.configure_preferences_controls(
            &device_name_field,
            &history_limit_field,
            &launch_at_login_checkbox,
            &share_checkbox,
            &prefer_remote_paste_checkbox,
            &discovery_checkbox,
        );

        self.ivars().preferences_window.replace(Some(window));
        self.ivars()
            .preferences_status_label
            .replace(Some(status_label));
        self.ivars()
            .preferences_save_button
            .replace(Some(save_button));
        self.ivars()
            .preferences_device_name_field
            .replace(Some(device_name_field));
        self.ivars()
            .preferences_history_limit_field
            .replace(Some(history_limit_field));
        self.ivars()
            .preferences_hotkey_field
            .replace(Some(hotkey_field));
        self.ivars()
            .preferences_hotkey_record_button
            .replace(Some(hotkey_record_button));
        self.ivars()
            .preferences_hotkey_preview_label
            .replace(Some(hotkey_preview_label));
        self.ivars()
            .preferences_launch_at_login_checkbox
            .replace(Some(launch_at_login_checkbox));
        self.ivars()
            .preferences_share_checkbox
            .replace(Some(share_checkbox));
        self.ivars()
            .preferences_prefer_remote_paste_checkbox
            .replace(Some(prefer_remote_paste_checkbox));
        self.ivars()
            .preferences_discovery_checkbox
            .replace(Some(discovery_checkbox));
        self.ivars()
            .preferences_devices_table
            .replace(Some(devices_table));
        self.ivars()
            .preferences_devices_context_menu
            .replace(Some(devices_context_menu));
        self.ivars()
            .preferences_devices_context_action_item
            .replace(Some(devices_context_action_item));
        self.ivars()
            .preferences_devices_context_properties_item
            .replace(Some(devices_context_properties_item));
    }

    fn build_general_preferences_tab(
        &self,
        mtm: MainThreadMarker,
        size: NSSize,
    ) -> (
        Retained<NSView>,
        Retained<NSTextField>,
        Retained<NSTextField>,
        Retained<NSButton>,
    ) {
        let view = NSView::initWithFrame(
            NSView::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), size),
        );
        let inset = 18.0;
        let label_width = 136.0;
        let field_x = inset + label_width + 12.0;
        let field_width = size.width - field_x - inset;

        let device_name_label = make_field_label(
            mtm,
            "设备名称",
            NSRect::new(
                NSPoint::new(inset, size.height - 42.0),
                NSSize::new(label_width, 20.0),
            ),
        );
        let device_name_field = make_input_field(
            mtm,
            NSRect::new(
                NSPoint::new(field_x, size.height - 48.0),
                NSSize::new(field_width, 24.0),
            ),
            "我的设备",
        );

        let history_limit_label = make_field_label(
            mtm,
            "历史数量上限",
            NSRect::new(
                NSPoint::new(inset, size.height - 84.0),
                NSSize::new(label_width, 20.0),
            ),
        );
        let history_limit_field = make_input_field(
            mtm,
            NSRect::new(
                NSPoint::new(field_x, size.height - 90.0),
                NSSize::new(132.0, 24.0),
            ),
            "120",
        );
        let launch_at_login_checkbox = make_checkbox(
            mtm,
            "开机自动启动",
            NSRect::new(
                NSPoint::new(inset, size.height - 126.0),
                NSSize::new(220.0, 18.0),
            ),
        );

        view.addSubview(&device_name_label);
        view.addSubview(&device_name_field);
        view.addSubview(&history_limit_label);
        view.addSubview(&history_limit_field);
        view.addSubview(&launch_at_login_checkbox);

        (
            view,
            device_name_field,
            history_limit_field,
            launch_at_login_checkbox,
        )
    }

    fn build_hotkey_preferences_tab(
        &self,
        mtm: MainThreadMarker,
        size: NSSize,
    ) -> (
        Retained<NSView>,
        Retained<NSTextField>,
        Retained<NSButton>,
        Retained<NSTextField>,
    ) {
        let view = NSView::initWithFrame(
            NSView::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), size),
        );
        let inset = 18.0;
        let label_width = 136.0;
        let field_x = inset + label_width + 12.0;
        let field_width = size.width - field_x - inset;
        let record_button_width = 68.0;
        let record_gap = 8.0;
        let hotkey_label = make_field_label(
            mtm,
            "历史弹窗快捷键",
            NSRect::new(
                NSPoint::new(inset, size.height - 42.0),
                NSSize::new(label_width, 20.0),
            ),
        );
        let hotkey_field = make_input_field(
            mtm,
            NSRect::new(
                NSPoint::new(field_x, size.height - 48.0),
                NSSize::new(field_width - record_button_width - record_gap, 24.0),
            ),
            "CmdOrCtrl+Shift+V",
        );
        hotkey_field.setEditable(false);
        hotkey_field.setSelectable(false);
        let hotkey_record_button = unsafe {
            NSButton::buttonWithTitle_target_action(
                ns_string!(HOTKEY_CAPTURE_BUTTON_IDLE),
                Some(self),
                Some(sel!(toggleHotkeyCapture:)),
                mtm,
            )
        };
        hotkey_record_button.setFrame(NSRect::new(
            NSPoint::new(
                field_x + field_width - record_button_width,
                size.height - 50.0,
            ),
            NSSize::new(record_button_width, 28.0),
        ));
        let hotkey_preview_label = make_secondary_label(
            mtm,
            HOTKEY_CAPTURE_HINT_IDLE,
            NSRect::new(
                NSPoint::new(field_x, size.height - 76.0),
                NSSize::new(field_width, 18.0),
            ),
        );

        view.addSubview(&hotkey_label);
        view.addSubview(&hotkey_field);
        view.addSubview(&hotkey_record_button);
        view.addSubview(&hotkey_preview_label);

        (
            view,
            hotkey_field,
            hotkey_record_button,
            hotkey_preview_label,
        )
    }

    fn build_sharing_preferences_tab(
        &self,
        mtm: MainThreadMarker,
        size: NSSize,
    ) -> (
        Retained<NSView>,
        Retained<NSButton>,
        Retained<NSButton>,
        Retained<NSButton>,
    ) {
        let view = NSView::initWithFrame(
            NSView::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), size),
        );
        let inset = 18.0;

        let share_checkbox = make_checkbox(
            mtm,
            "共享本机剪切板历史",
            NSRect::new(
                NSPoint::new(inset, size.height - 48.0),
                NSSize::new(280.0, 18.0),
            ),
        );
        let prefer_remote_paste_checkbox = make_checkbox(
            mtm,
            "粘贴时优先使用最新共享内容",
            NSRect::new(
                NSPoint::new(inset, size.height - 78.0),
                NSSize::new(320.0, 18.0),
            ),
        );
        let discovery_checkbox = make_checkbox(
            mtm,
            "启用局域网设备发现",
            NSRect::new(
                NSPoint::new(inset, size.height - 108.0),
                NSSize::new(280.0, 18.0),
            ),
        );

        view.addSubview(&share_checkbox);
        view.addSubview(&prefer_remote_paste_checkbox);
        view.addSubview(&discovery_checkbox);

        (
            view,
            share_checkbox,
            prefer_remote_paste_checkbox,
            discovery_checkbox,
        )
    }

    fn build_devices_preferences_tab(
        &self,
        mtm: MainThreadMarker,
        size: NSSize,
    ) -> DevicesPreferencesTabViews {
        let view = NSView::initWithFrame(
            NSView::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), size),
        );
        let inset = 18.0;

        let scroll_frame = NSRect::new(
            NSPoint::new(inset, 62.0),
            NSSize::new(size.width - inset * 2.0, size.height - 80.0),
        );
        let devices_box = make_background_box(mtm, scroll_frame);
        let scroll_view = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), scroll_frame);
        scroll_view.setDrawsBackground(false);
        scroll_view.setBorderType(objc2_app_kit::NSBorderType::NoBorder);
        scroll_view.setHasVerticalScroller(true);

        let table_view = NSTableView::initWithFrame(
            NSTableView::alloc(mtm),
            NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(scroll_frame.size.width, scroll_frame.size.height),
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
        table_view.setRowSizeStyle(NSTableViewRowSizeStyle::Default);
        table_view.setRowHeight(PREFERENCES_DEVICE_ROW_HEIGHT);
        table_view.setIntercellSpacing(NSSize::new(0.0, 0.0));
        table_view.setUsesAutomaticRowHeights(false);
        table_view.setAllowsMultipleSelection(true);
        unsafe {
            table_view.setDataSource(Some(ProtocolObject::from_ref(self)));
            table_view.setDelegate(Some(ProtocolObject::from_ref(self)));
        }

        let column =
            NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), ns_string!("devices"));
        column.setEditable(false);
        column.setResizingMask(NSTableColumnResizingOptions::AutoresizingMask);
        column.setWidth(scroll_frame.size.width);
        table_view.addTableColumn(&column);

        let devices_context_menu =
            NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("设备操作"));
        devices_context_menu.setDelegate(Some(ProtocolObject::from_ref(self)));
        let action_item = make_menu_item(
            mtm,
            "信任设备",
            Some(self),
            Some(sel!(trustSelectedDevice:)),
            "",
        );
        let properties_item = make_menu_item(
            mtm,
            "设备属性",
            Some(self),
            Some(sel!(showSelectedDeviceProperties:)),
            "",
        );
        action_item.setEnabled(false);
        properties_item.setEnabled(false);
        devices_context_menu.addItem(&action_item);
        devices_context_menu.addItem(&properties_item);
        unsafe {
            table_view.setMenu(Some(&devices_context_menu));
        }
        scroll_view.setDocumentView(Some(&table_view));

        view.addSubview(&devices_box);
        view.addSubview(&scroll_view);

        (
            view,
            table_view,
            devices_context_menu,
            action_item,
            properties_item,
        )
    }

    fn configure_preferences_controls(
        &self,
        device_name_field: &NSTextField,
        history_limit_field: &NSTextField,
        launch_at_login_checkbox: &NSButton,
        share_checkbox: &NSButton,
        prefer_remote_paste_checkbox: &NSButton,
        discovery_checkbox: &NSButton,
    ) {
        unsafe {
            device_name_field.setDelegate(Some(ProtocolObject::from_ref(self)));
            history_limit_field.setDelegate(Some(ProtocolObject::from_ref(self)));
            launch_at_login_checkbox.setTarget(Some(self));
            launch_at_login_checkbox.setAction(Some(sel!(preferencesChanged:)));
            share_checkbox.setTarget(Some(self));
            share_checkbox.setAction(Some(sel!(preferencesChanged:)));
            prefer_remote_paste_checkbox.setTarget(Some(self));
            prefer_remote_paste_checkbox.setAction(Some(sel!(preferencesChanged:)));
            discovery_checkbox.setTarget(Some(self));
            discovery_checkbox.setAction(Some(sel!(preferencesChanged:)));
        }
    }

    pub(in crate::platform::macos) fn ensure_preferences_window(&self) {
        if self.preferences_window().is_none() {
            self.install_preferences_window(self.mtm());
        }
    }

    pub(in crate::platform::macos) fn show_preferences_window(&self) {
        self.hide_panel(false);
        self.ensure_preferences_window();
        self.stop_hotkey_capture(false);
        let status_override = self
            .ivars()
            .controller
            .borrow_mut()
            .set_preferences_visible(true)
            .err()
            .map(|error| format!("偏好设置已打开，但设备服务启动失败: {error}"));
        self.populate_preferences_window(status_override);

        let mtm = self.mtm();
        let app = NSApplication::sharedApplication(mtm);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);

        if let Some(window) = self.preferences_window() {
            window.makeKeyAndOrderFront(None);
        }
    }

    pub(in crate::platform::macos) fn hide_preferences_window(&self) {
        self.stop_hotkey_capture(false);
        let _ = self
            .ivars()
            .controller
            .borrow_mut()
            .set_preferences_visible(false);
        self.release_preferences_window();
    }

    fn release_preferences_window(&self) {
        let window = self.ivars().preferences_window.borrow_mut().take();
        if let Some(window) = window.as_ref() {
            window.orderOut(None);
            window.setContentView(None);
            window.close();
        }

        self.ivars().preferences_baseline.replace(None);
        self.ivars().preferences_status_label.replace(None);
        self.ivars().preferences_save_button.replace(None);
        self.ivars().preferences_device_name_field.replace(None);
        self.ivars().preferences_history_limit_field.replace(None);
        self.ivars().preferences_hotkey_field.replace(None);
        self.ivars().preferences_hotkey_record_button.replace(None);
        self.ivars().preferences_hotkey_preview_label.replace(None);
        self.ivars()
            .preferences_launch_at_login_checkbox
            .replace(None);
        self.ivars().preferences_share_checkbox.replace(None);
        self.ivars()
            .preferences_prefer_remote_paste_checkbox
            .replace(None);
        self.ivars().preferences_discovery_checkbox.replace(None);
        self.ivars().preferences_devices_table.replace(None);
        let mut device_rows = self.ivars().preferences_devices_rows.borrow_mut();
        device_rows.clear();
        device_rows.shrink_to_fit();
        drop(device_rows);
        self.ivars().preferences_selected_device_id.replace(None);
        self.ivars()
            .preferences_selected_device_ids
            .replace(Vec::new());
        self.ivars().preferences_devices_context_menu.replace(None);
        self.ivars()
            .preferences_devices_context_action_item
            .replace(None);
        self.ivars()
            .preferences_devices_context_properties_item
            .replace(None);
    }
}
