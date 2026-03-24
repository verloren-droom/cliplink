use super::{
    ui::{PREFERENCES_DEVICE_ROW_HEIGHT, PREFERENCES_HEIGHT, PREFERENCES_WIDTH},
    *,
};
use std::ptr::{NonNull, null_mut};

use block2::RcBlock;
use global_hotkey::hotkey::HotKey;
use objc2::sel;
use objc2_foundation::ns_string;

use crate::{
    constants::limits::{MAX_DEVICE_NAME_CHARS, MAX_HISTORY_LIMIT},
    platform::macos::hotkey::{format_hotkey_for_display, hotkey_from_event},
    platform::macos::widgets::{
        make_background_box, make_checkbox, make_field_label, make_input_field,
        make_secondary_label,
    },
};

const HOTKEY_CAPTURE_HINT_IDLE: &str = "点击“录制”后直接按下新的组合键";
const HOTKEY_CAPTURE_HINT_ACTIVE: &str = "请按下新的快捷键，按 Esc 取消";
const HOTKEY_CAPTURE_BUTTON_IDLE: &str = "录制";
const HOTKEY_CAPTURE_BUTTON_ACTIVE: &str = "取消";

impl AppDelegate {
    pub(super) fn install_preferences_window(&self, mtm: MainThreadMarker) {
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
        let (devices_view, devices_table, trust_device_button, revoke_device_button) =
            self.build_devices_preferences_tab(mtm, tab_size);

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
            .preferences_trust_device_button
            .replace(Some(trust_device_button));
        self.ivars()
            .preferences_revoke_device_button
            .replace(Some(revoke_device_button));
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
    ) -> (
        Retained<NSView>,
        Retained<NSTableView>,
        Retained<NSButton>,
        Retained<NSButton>,
    ) {
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
        scroll_view.setDocumentView(Some(&table_view));

        let trust_device_button = unsafe {
            NSButton::buttonWithTitle_target_action(
                ns_string!("信任设备"),
                Some(self),
                Some(sel!(trustSelectedDevice:)),
                mtm,
            )
        };
        trust_device_button.sizeToFit();
        trust_device_button.setFrameOrigin(NSPoint::new(inset, 18.0));
        trust_device_button.setEnabled(false);

        let revoke_device_button = unsafe {
            NSButton::buttonWithTitle_target_action(
                ns_string!("移除信任"),
                Some(self),
                Some(sel!(revokeSelectedDevice:)),
                mtm,
            )
        };
        revoke_device_button.sizeToFit();
        revoke_device_button.setFrameOrigin(NSPoint::new(inset + 96.0, 18.0));
        revoke_device_button.setEnabled(false);

        view.addSubview(&devices_box);
        view.addSubview(&scroll_view);
        view.addSubview(&trust_device_button);
        view.addSubview(&revoke_device_button);

        (view, table_view, trust_device_button, revoke_device_button)
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

    pub(super) fn ensure_preferences_window(&self) {
        if self.preferences_window().is_none() {
            self.install_preferences_window(self.mtm());
        }
    }

    pub(super) fn show_preferences_window(&self) {
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

    pub(super) fn hide_preferences_window(&self) {
        self.stop_hotkey_capture(false);
        let _ = self
            .ivars()
            .controller
            .borrow_mut()
            .set_preferences_visible(false);
        self.release_preferences_window();
    }

    pub(super) fn populate_preferences_window(&self, status_override: Option<String>) {
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

    pub(super) fn refresh_preferences_runtime_state(&self) {
        let snapshot = self.ivars().controller.borrow().settings_snapshot();
        self.replace_preferences_devices(snapshot.devices);
        if let Some(status) = snapshot.status {
            self.set_preferences_status(status);
        }
        self.refresh_hotkey_capture_ui(self.hotkey_capture_active());
    }

    pub(super) fn save_preferences(&self) {
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

        let result = self.ivars().controller.borrow_mut().apply_settings(update);
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

    pub(super) fn sync_preferences_form_state(&self) {
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

    pub(super) fn set_preferences_status(&self, message: impl Into<String>) {
        let Some(label) = self.preferences_status_label() else {
            return;
        };
        label.setStringValue(&NSString::from_str(&message.into()));
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
        self.ivars().preferences_devices_rows.borrow_mut().clear();
        self.ivars().preferences_selected_device_id.replace(None);
        self.ivars().preferences_trust_device_button.replace(None);
        self.ivars().preferences_revoke_device_button.replace(None);
    }

    pub(super) fn toggle_hotkey_capture(&self) {
        if self.hotkey_capture_active() {
            self.stop_hotkey_capture(true);
        } else {
            self.start_hotkey_capture();
        }
    }

    fn start_hotkey_capture(&self) {
        self.stop_hotkey_capture(false);

        let delegate_ptr = self as *const AppDelegate;
        let block = RcBlock::new(move |event_ptr: NonNull<NSEvent>| -> *mut NSEvent {
            let delegate = unsafe { &*delegate_ptr };
            delegate.handle_hotkey_capture_event(event_ptr);
            null_mut()
        });

        let monitor = unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block)
        };
        *self.ivars().hotkey_capture_block.borrow_mut() = Some(block);
        *self.ivars().hotkey_capture_monitor.borrow_mut() = monitor;
        self.refresh_hotkey_capture_ui(true);
        self.set_preferences_status(HOTKEY_CAPTURE_HINT_ACTIVE);
    }

    fn stop_hotkey_capture(&self, cancelled: bool) {
        if let Some(monitor) = self.ivars().hotkey_capture_monitor.borrow_mut().take() {
            unsafe { NSEvent::removeMonitor(&monitor) };
        }
        self.ivars().hotkey_capture_block.borrow_mut().take();
        self.refresh_hotkey_capture_ui(false);
        if cancelled {
            self.set_preferences_status("已取消快捷键录制。");
        }
    }

    fn hotkey_capture_active(&self) -> bool {
        self.ivars().hotkey_capture_monitor.borrow().is_some()
    }

    fn refresh_hotkey_capture_ui(&self, active: bool) {
        if let Some(button) = self.preferences_hotkey_record_button() {
            button.setTitle(&NSString::from_str(if active {
                HOTKEY_CAPTURE_BUTTON_ACTIVE
            } else {
                HOTKEY_CAPTURE_BUTTON_IDLE
            }));
        }
        if let Some(label) = self.preferences_hotkey_preview_label() {
            label.setStringValue(&NSString::from_str(if active {
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
    }

    fn handle_hotkey_capture_event(&self, event_ptr: NonNull<NSEvent>) {
        let event = unsafe { event_ptr.as_ref() };

        if event.isARepeat() {
            return;
        }

        match hotkey_from_event(event) {
            Ok(None) => {
                self.stop_hotkey_capture(true);
            }
            Ok(Some(hotkey)) => {
                self.ivars()
                    .preferences_hotkey_value
                    .replace(hotkey.to_string());
                self.stop_hotkey_capture(false);
                self.sync_preferences_form_state();
                self.set_preferences_status("已更新快捷键，点击保存后生效。");
            }
            Err(message) => {
                self.set_preferences_status(message);
            }
        }
    }

    pub(super) fn sync_selected_device_from_table(&self) {
        let Some(table_view) = self.preferences_devices_table() else {
            return;
        };
        let row = table_view.selectedRow();
        let selected_device_id = (row >= 0).then_some(row as usize).and_then(|row| {
            self.ivars()
                .preferences_devices_rows
                .borrow()
                .get(row)
                .map(|entry| entry.device_id.clone())
        });
        self.ivars()
            .preferences_selected_device_id
            .replace(selected_device_id);
        self.refresh_preferences_device_actions();
    }

    fn replace_preferences_devices(&self, devices: Vec<crate::controller::SettingsDeviceEntry>) {
        let previous_selection = self.ivars().preferences_selected_device_id.borrow().clone();
        *self.ivars().preferences_devices_rows.borrow_mut() = devices;

        let target_row = {
            let rows = self.ivars().preferences_devices_rows.borrow();
            previous_selection
                .as_ref()
                .and_then(|selected| rows.iter().position(|entry| &entry.device_id == selected))
                .or_else(|| (!rows.is_empty()).then_some(0))
        };

        let Some(table_view) = self.preferences_devices_table() else {
            self.ivars()
                .preferences_selected_device_id
                .replace(target_row.and_then(|row| {
                    self.ivars()
                        .preferences_devices_rows
                        .borrow()
                        .get(row)
                        .map(|entry| entry.device_id.clone())
                }));
            self.refresh_preferences_device_actions();
            return;
        };

        table_view.reloadData();

        if let Some(row) = target_row {
            let device_id = self
                .ivars()
                .preferences_devices_rows
                .borrow()
                .get(row)
                .map(|entry| entry.device_id.clone());
            self.ivars()
                .preferences_selected_device_id
                .replace(device_id);
            let indexes = NSIndexSet::indexSetWithIndex(row as NSUInteger);
            table_view.selectRowIndexes_byExtendingSelection(&indexes, false);
            table_view.scrollRowToVisible(row as NSInteger);
        } else {
            self.ivars().preferences_selected_device_id.replace(None);
            unsafe {
                table_view.deselectAll(None);
            }
        }

        self.refresh_preferences_device_actions();
    }

    fn refresh_preferences_device_actions(&self) {
        let selected_entry = self
            .ivars()
            .preferences_selected_device_id
            .borrow()
            .as_ref()
            .and_then(|selected| {
                self.ivars()
                    .preferences_devices_rows
                    .borrow()
                    .iter()
                    .find(|entry| entry.device_id == *selected)
                    .cloned()
            });

        let (can_trust, can_revoke) = selected_entry
            .as_ref()
            .map(|entry| (!entry.is_trusted && entry.is_online, entry.is_trusted))
            .unwrap_or((false, false));

        if let Some(button) = self.preferences_trust_device_button() {
            button.setEnabled(can_trust);
        }
        if let Some(button) = self.preferences_revoke_device_button() {
            button.setEnabled(can_revoke);
        }
    }
}
