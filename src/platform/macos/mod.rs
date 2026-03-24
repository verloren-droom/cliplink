#![deny(unsafe_op_in_unsafe_fn)]

use std::{
    cell::{OnceCell, RefCell},
    ptr::NonNull,
};

use block2::RcBlock;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
use objc2::{
    DefinedClass, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject, Sel},
    sel,
};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSBackingStoreType,
    NSButton, NSColor, NSControl, NSControlTextEditingDelegate, NSEvent, NSEventMask,
    NSEventModifierFlags, NSEventType, NSFloatingWindowLevel, NSFont, NSImage, NSImageScaling,
    NSLineBreakMode, NSMenu, NSMenuItem, NSPanel, NSPopUpMenuWindowLevel, NSScreen, NSScrollView,
    NSSearchField, NSSearchFieldDelegate, NSStatusBar, NSStatusItem, NSStatusItemBehavior,
    NSTabView, NSTabViewItem, NSTabViewType, NSTableCellView, NSTableColumn,
    NSTableColumnResizingOptions, NSTableView, NSTableViewColumnAutoresizingStyle,
    NSTableViewDataSource, NSTableViewDelegate, NSTableViewRowSizeStyle,
    NSTableViewSelectionHighlightStyle, NSTableViewStyle, NSTextField, NSTextFieldDelegate,
    NSTextView, NSVariableStatusItemLength, NSView, NSWindow, NSWindowCollectionBehavior,
    NSWindowDelegate, NSWindowStyleMask, NSWindowTitleVisibility, NSWorkspace,
};
use objc2_foundation::{
    MainThreadMarker, NSIndexSet, NSInteger, NSNotification, NSObject, NSObjectProtocol, NSPoint,
    NSRect, NSSize, NSString, NSTimer, NSUInteger,
};
use uuid::Uuid;

use crate::{
    controller::{AppController, HistoryRow, SettingsDeviceEntry, SettingsUpdate},
    core::{clipboard::ClipboardBackend, error::AppResult},
    platform::PlatformResult,
};

mod about;
mod autostart;
mod clipboard;
mod hotkey;
mod keychain;
mod panel;
mod paste;
mod preferences;
mod tray;
mod ui;
mod widgets;

use self::ui::{PREFERENCES_DEVICE_ROW_HEIGHT, ROW_HEIGHT};

type HotKeyMonitorBlock = RcBlock<dyn Fn(NonNull<NSEvent>) -> *mut NSEvent>;

struct AppDelegateIvars {
    controller: RefCell<AppController>,
    search_query: RefCell<String>,
    filtered_rows: RefCell<Vec<HistoryRow>>,
    selected_row: RefCell<Option<usize>>,
    preferences_baseline: RefCell<Option<SettingsUpdate>>,
    preferences_hotkey_value: RefCell<String>,
    menu_history_ids: RefCell<Vec<Uuid>>,
    previous_frontmost_bundle_id: RefCell<Option<String>>,
    status_item: OnceCell<Retained<NSStatusItem>>,
    status_menu: OnceCell<Retained<NSMenu>>,
    history_context_menu: RefCell<Option<Retained<NSMenu>>>,
    panel: RefCell<Option<Retained<NSPanel>>>,
    about_window: RefCell<Option<Retained<NSWindow>>>,
    search_field: RefCell<Option<Retained<NSSearchField>>>,
    table_view: RefCell<Option<Retained<NSTableView>>>,
    preferences_window: RefCell<Option<Retained<NSWindow>>>,
    preferences_status_label: RefCell<Option<Retained<NSTextField>>>,
    preferences_save_button: RefCell<Option<Retained<NSButton>>>,
    preferences_device_name_field: RefCell<Option<Retained<NSTextField>>>,
    preferences_history_limit_field: RefCell<Option<Retained<NSTextField>>>,
    preferences_hotkey_field: RefCell<Option<Retained<NSTextField>>>,
    preferences_hotkey_record_button: RefCell<Option<Retained<NSButton>>>,
    preferences_hotkey_preview_label: RefCell<Option<Retained<NSTextField>>>,
    preferences_launch_at_login_checkbox: RefCell<Option<Retained<NSButton>>>,
    preferences_share_checkbox: RefCell<Option<Retained<NSButton>>>,
    preferences_prefer_remote_paste_checkbox: RefCell<Option<Retained<NSButton>>>,
    preferences_discovery_checkbox: RefCell<Option<Retained<NSButton>>>,
    preferences_devices_table: RefCell<Option<Retained<NSTableView>>>,
    preferences_devices_rows: RefCell<Vec<SettingsDeviceEntry>>,
    preferences_selected_device_id: RefCell<Option<String>>,
    preferences_trust_device_button: RefCell<Option<Retained<NSButton>>>,
    preferences_revoke_device_button: RefCell<Option<Retained<NSButton>>>,
    hotkey_manager: RefCell<Option<GlobalHotKeyManager>>,
    hotkey: RefCell<Option<HotKey>>,
    hotkey_capture_monitor: RefCell<Option<Retained<AnyObject>>>,
    hotkey_capture_block: RefCell<Option<HotKeyMonitorBlock>>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = AppDelegateIvars]
    struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn application_did_finish_launching(&self, _notification: &NSNotification) {
            let mtm = self.mtm();
            let app = NSApplication::sharedApplication(mtm);
            app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

            self.install_status_item(mtm);
            let launch_at_login = self.ivars().controller.borrow().launch_at_login_enabled();
            let autostart_sync = match autostart::launch_at_login_enabled() {
                Ok(enabled) if enabled == launch_at_login => Ok(()),
                Ok(_) => autostart::set_launch_at_login(launch_at_login),
                Err(error) => Err(error),
            };
            if let Err(error) = autostart_sync {
                self.ivars()
                    .controller
                    .borrow_mut()
                    .report_status(format!("开机自动启动同步失败: {error}"));
            }
            self.install_hotkey();
            self.install_timer();
        }
    }

    unsafe impl NSWindowDelegate for AppDelegate {
        #[unsafe(method(windowDidResignKey:))]
        fn window_did_resign_key(&self, notification: &NSNotification) {
            let Some(object) = notification.object() else {
                return;
            };
            let window = unsafe { &*(Retained::as_ptr(&object) as *const NSWindow) };
            if self.window_matches_panel(window) {
                self.hide_panel(true);
            }
        }

        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> objc2::runtime::Bool {
            if self.window_matches_panel(_sender) {
                self.hide_panel(true);
                false.into()
            } else if self.window_matches_preferences(_sender) {
                self.hide_preferences_window();
                false.into()
            } else if self.window_matches_about(_sender) {
                self.hide_about_window();
                false.into()
            } else {
                true.into()
            }
        }
    }

    unsafe impl NSControlTextEditingDelegate for AppDelegate {
        #[unsafe(method(controlTextDidChange:))]
        fn control_text_did_change(&self, notification: &NSNotification) {
            let Some(object) = notification.object() else {
                return;
            };
            let object = &*object;

            if self.object_matches_search_field(object) {
                let Some(search_field) = self.search_field() else {
                    return;
                };

                self.ivars()
                    .search_query
                    .replace(search_field.stringValue().to_string());
                self.reload_filtered_rows_revealing_selection();
                return;
            }

            if self.object_matches_preferences_field(object) {
                self.sync_preferences_form_state();
            }
        }

        #[unsafe(method(control:textView:doCommandBySelector:))]
        unsafe fn control_text_view_do_command_by_selector(
            &self,
            _control: &NSControl,
            _text_view: &NSTextView,
            command_selector: Sel,
        ) -> bool {
            if command_selector == sel!(moveDown:) {
                self.move_selection(1);
                true
            } else if command_selector == sel!(moveUp:) {
                self.move_selection(-1);
                true
            } else if command_selector == sel!(insertNewline:) {
                self.activate_selected();
                true
            } else if command_selector == sel!(cancelOperation:) {
                self.hide_panel(true);
                true
            } else {
                false
            }
        }
    }

    unsafe impl NSTextFieldDelegate for AppDelegate {}
    unsafe impl NSSearchFieldDelegate for AppDelegate {}

    unsafe impl NSTableViewDataSource for AppDelegate {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn number_of_rows_in_table_view(&self, table_view: &NSTableView) -> NSInteger {
            if self.table_matches_preferences_devices_table(table_view) {
                self.ivars().preferences_devices_rows.borrow().len() as NSInteger
            } else {
                self.ivars().filtered_rows.borrow().len() as NSInteger
            }
        }
    }

    unsafe impl NSTableViewDelegate for AppDelegate {
        #[unsafe(method(tableView:heightOfRow:))]
        fn table_view_height_of_row(&self, table_view: &NSTableView, _row: NSInteger) -> f64 {
            if self.table_matches_preferences_devices_table(table_view) {
                PREFERENCES_DEVICE_ROW_HEIGHT
            } else {
                ROW_HEIGHT
            }
        }

        #[unsafe(method(tableViewSelectionDidChange:))]
        fn table_view_selection_did_change(&self, notification: &NSNotification) {
            let Some(object) = notification.object() else {
                return;
            };
            let object = &*object;

            if self.object_matches_history_table(object) {
                self.sync_selected_row_from_table();
            } else if self.object_matches_preferences_devices_table(object) {
                self.sync_selected_device_from_table();
            }
        }

        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn table_view_view_for_table_column_row(
            &self,
            table_view: &NSTableView,
            _table_column: Option<&NSTableColumn>,
            row_index: NSInteger,
        ) -> Option<Retained<objc2_app_kit::NSView>> {
            let row_index = row_index.max(0) as usize;
            if self.table_matches_preferences_devices_table(table_view) {
                self.ivars()
                    .preferences_devices_rows
                    .borrow()
                    .get(row_index)
                    .cloned()
                    .map(|device_row| {
                        widgets::make_preferences_device_row_view(
                            self.mtm(),
                            &device_row,
                            table_view.frame().size.width,
                        )
                        .into_super()
                    })
            } else {
                self.ivars()
                    .filtered_rows
                    .borrow()
                    .get(row_index)
                    .cloned()
                    .map(|history_row| {
                        let is_selected =
                            self.ivars().selected_row.borrow().as_ref().copied() == Some(row_index);
                        widgets::make_history_row_view(
                            self.mtm(),
                            &history_row,
                            table_view.frame().size.width,
                            is_selected,
                            self,
                            row_index,
                        )
                        .into_super()
                    })
            }
        }
    }

    impl AppDelegate {
        #[unsafe(method(statusItemAction:))]
        fn status_item_action(&self, _sender: Option<&AnyObject>) {
            self.handle_status_item_click();
        }

        #[unsafe(method(tick:))]
        fn tick_action(&self, _timer: &NSTimer) {
            while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
                if event.state == HotKeyState::Released {
                    self.toggle_panel();
                }
            }

            let outcome = {
                let mut controller = self.ivars().controller.borrow_mut();
                controller.tick()
            };

            if outcome.history_changed && self.panel_visible() {
                self.reload_filtered_rows();
            }
            if outcome.preferences_changed()
                && self
                    .preferences_window()
                    .is_some_and(|window| window.isVisible())
            {
                self.refresh_preferences_runtime_state();
            }
        }

        #[unsafe(method(activateSelected:))]
        fn activate_selected_action(&self, sender: Option<&AnyObject>) {
            self.activate_clicked_or_selected(sender);
        }

        #[unsafe(method(deleteHistoryItem:))]
        fn delete_history_item_action(&self, sender: Option<&AnyObject>) {
            let row = sender
                .map(|sender| unsafe { msg_send![sender, tag] })
                .filter(|tag: &NSInteger| *tag >= 0)
                .map(|tag| tag as usize)
                .or_else(|| self.context_history_row())
                .or_else(|| self.ivars().selected_row.borrow().as_ref().copied());

            if let Some(row) = row {
                self.delete_history_row(row);
            }
        }

        #[unsafe(method(toggleHistoryItemPin:))]
        fn toggle_history_item_pin_action(&self, sender: Option<&AnyObject>) {
            let row = sender
                .map(|sender| unsafe { msg_send![sender, tag] })
                .filter(|tag: &NSInteger| *tag >= 0)
                .map(|tag| tag as usize)
                .or_else(|| self.context_history_row())
                .or_else(|| self.ivars().selected_row.borrow().as_ref().copied());

            let Some(row) = row else {
                return;
            };
            self.toggle_history_row_pin(row);
        }

        #[unsafe(method(clearHistory:))]
        fn clear_history_action(&self, _sender: Option<&AnyObject>) {
            let _ = self.ivars().controller.borrow_mut().clear_history();
            self.reload_filtered_rows();
            self.refresh_preferences_runtime_state();
        }

        #[unsafe(method(openPreferences:))]
        fn open_preferences_action(&self, _sender: Option<&AnyObject>) {
            self.show_preferences_window();
        }

        #[unsafe(method(openAbout:))]
        fn open_about_action(&self, _sender: Option<&AnyObject>) {
            self.show_about_window();
        }

        #[unsafe(method(closeAbout:))]
        fn close_about_action(&self, _sender: Option<&AnyObject>) {
            self.hide_about_window();
        }

        #[unsafe(method(activateMenuHistory:))]
        fn activate_menu_history_action(&self, sender: Option<&AnyObject>) {
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

            if self.ivars().controller.borrow_mut().copy_item(id).ok() == Some(true) {
                self.trigger_immediate_paste();
            }
        }

        #[unsafe(method(savePreferences:))]
        fn save_preferences_action(&self, _sender: Option<&AnyObject>) {
            self.save_preferences();
        }

        #[unsafe(method(closePreferences:))]
        fn close_preferences_action(&self, _sender: Option<&AnyObject>) {
            self.hide_preferences_window();
        }

        #[unsafe(method(preferencesChanged:))]
        fn preferences_changed_action(&self, _sender: Option<&AnyObject>) {
            self.sync_preferences_form_state();
        }

        #[unsafe(method(toggleHotkeyCapture:))]
        fn toggle_hotkey_capture_action(&self, _sender: Option<&AnyObject>) {
            self.toggle_hotkey_capture();
        }

        #[unsafe(method(trustSelectedDevice:))]
        fn trust_selected_device_action(&self, _sender: Option<&AnyObject>) {
            let Some(device_id) = self
                .ivars()
                .preferences_selected_device_id
                .borrow()
                .clone()
            else {
                self.set_preferences_status("请选择一个在线设备。");
                return;
            };

            match self.ivars().controller.borrow_mut().trust_device(&device_id) {
                Ok(true) => self.refresh_preferences_runtime_state(),
                Ok(false) => self.set_preferences_status("仅支持信任当前在线设备。"),
                Err(error) => self.set_preferences_status(format!("信任设备失败: {error}")),
            }
        }

        #[unsafe(method(revokeSelectedDevice:))]
        fn revoke_selected_device_action(&self, _sender: Option<&AnyObject>) {
            let Some(device_id) = self
                .ivars()
                .preferences_selected_device_id
                .borrow()
                .clone()
            else {
                self.set_preferences_status("请选择一个已信任设备。");
                return;
            };

            match self.ivars().controller.borrow_mut().revoke_device_trust(&device_id) {
                Ok(true) => self.refresh_preferences_runtime_state(),
                Ok(false) => self.set_preferences_status("未找到对应的信任设备。"),
                Err(error) => {
                    self.set_preferences_status(format!("移除信任设备失败: {error}"))
                }
            }
        }

        #[unsafe(method(quitApp:))]
        fn quit_app_action(&self, _sender: Option<&AnyObject>) {
            let app = NSApplication::sharedApplication(self.mtm());
            app.terminate(None);
        }
    }
);

impl AppDelegate {
    fn new(mtm: MainThreadMarker, controller: AppController) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(AppDelegateIvars {
            controller: RefCell::new(controller),
            search_query: RefCell::new(String::new()),
            filtered_rows: RefCell::new(Vec::new()),
            selected_row: RefCell::new(None),
            preferences_baseline: RefCell::new(None),
            preferences_hotkey_value: RefCell::new(String::new()),
            menu_history_ids: RefCell::new(Vec::new()),
            previous_frontmost_bundle_id: RefCell::new(None),
            status_item: OnceCell::new(),
            status_menu: OnceCell::new(),
            history_context_menu: RefCell::new(None),
            panel: RefCell::new(None),
            about_window: RefCell::new(None),
            search_field: RefCell::new(None),
            table_view: RefCell::new(None),
            preferences_window: RefCell::new(None),
            preferences_status_label: RefCell::new(None),
            preferences_save_button: RefCell::new(None),
            preferences_device_name_field: RefCell::new(None),
            preferences_history_limit_field: RefCell::new(None),
            preferences_hotkey_field: RefCell::new(None),
            preferences_hotkey_record_button: RefCell::new(None),
            preferences_hotkey_preview_label: RefCell::new(None),
            preferences_launch_at_login_checkbox: RefCell::new(None),
            preferences_share_checkbox: RefCell::new(None),
            preferences_prefer_remote_paste_checkbox: RefCell::new(None),
            preferences_discovery_checkbox: RefCell::new(None),
            preferences_devices_table: RefCell::new(None),
            preferences_devices_rows: RefCell::new(Vec::new()),
            preferences_selected_device_id: RefCell::new(None),
            preferences_trust_device_button: RefCell::new(None),
            preferences_revoke_device_button: RefCell::new(None),
            hotkey_manager: RefCell::new(None),
            hotkey: RefCell::new(None),
            hotkey_capture_monitor: RefCell::new(None),
            hotkey_capture_block: RefCell::new(None),
        });

        unsafe { msg_send![super(this), init] }
    }

    fn panel(&self) -> Option<Retained<NSPanel>> {
        self.ivars().panel.borrow().clone()
    }

    fn search_field(&self) -> Option<Retained<NSSearchField>> {
        self.ivars().search_field.borrow().clone()
    }

    fn about_window(&self) -> Option<Retained<NSWindow>> {
        self.ivars().about_window.borrow().clone()
    }

    fn table_view(&self) -> Option<Retained<NSTableView>> {
        self.ivars().table_view.borrow().clone()
    }

    fn status_item(&self) -> Option<&NSStatusItem> {
        self.ivars().status_item.get().map(|item| &**item)
    }

    fn status_menu(&self) -> Option<&NSMenu> {
        self.ivars().status_menu.get().map(|menu| &**menu)
    }

    fn preferences_window(&self) -> Option<Retained<NSWindow>> {
        self.ivars().preferences_window.borrow().clone()
    }

    fn preferences_status_label(&self) -> Option<Retained<NSTextField>> {
        self.ivars().preferences_status_label.borrow().clone()
    }

    fn preferences_save_button(&self) -> Option<Retained<NSButton>> {
        self.ivars().preferences_save_button.borrow().clone()
    }

    fn preferences_device_name_field(&self) -> Option<Retained<NSTextField>> {
        self.ivars().preferences_device_name_field.borrow().clone()
    }

    fn preferences_history_limit_field(&self) -> Option<Retained<NSTextField>> {
        self.ivars()
            .preferences_history_limit_field
            .borrow()
            .clone()
    }

    fn preferences_hotkey_field(&self) -> Option<Retained<NSTextField>> {
        self.ivars().preferences_hotkey_field.borrow().clone()
    }

    fn preferences_hotkey_record_button(&self) -> Option<Retained<NSButton>> {
        self.ivars()
            .preferences_hotkey_record_button
            .borrow()
            .clone()
    }

    fn preferences_hotkey_preview_label(&self) -> Option<Retained<NSTextField>> {
        self.ivars()
            .preferences_hotkey_preview_label
            .borrow()
            .clone()
    }

    fn preferences_launch_at_login_checkbox(&self) -> Option<Retained<NSButton>> {
        self.ivars()
            .preferences_launch_at_login_checkbox
            .borrow()
            .clone()
    }

    fn preferences_share_checkbox(&self) -> Option<Retained<NSButton>> {
        self.ivars().preferences_share_checkbox.borrow().clone()
    }

    fn preferences_prefer_remote_paste_checkbox(&self) -> Option<Retained<NSButton>> {
        self.ivars()
            .preferences_prefer_remote_paste_checkbox
            .borrow()
            .clone()
    }

    fn preferences_discovery_checkbox(&self) -> Option<Retained<NSButton>> {
        self.ivars().preferences_discovery_checkbox.borrow().clone()
    }

    fn preferences_devices_table(&self) -> Option<Retained<NSTableView>> {
        self.ivars().preferences_devices_table.borrow().clone()
    }

    fn preferences_trust_device_button(&self) -> Option<Retained<NSButton>> {
        self.ivars()
            .preferences_trust_device_button
            .borrow()
            .clone()
    }

    fn preferences_revoke_device_button(&self) -> Option<Retained<NSButton>> {
        self.ivars()
            .preferences_revoke_device_button
            .borrow()
            .clone()
    }

    fn object_matches_search_field(&self, object: &AnyObject) -> bool {
        self.search_field().is_some_and(|field| {
            std::ptr::eq(
                object as *const AnyObject,
                field.as_ref() as *const NSSearchField as *const AnyObject,
            )
        })
    }

    fn object_matches_history_table(&self, object: &AnyObject) -> bool {
        self.table_view().is_some_and(|table| {
            std::ptr::eq(
                object as *const AnyObject,
                table.as_ref() as *const NSTableView as *const AnyObject,
            )
        })
    }

    fn object_matches_preferences_devices_table(&self, object: &AnyObject) -> bool {
        self.preferences_devices_table().is_some_and(|table| {
            std::ptr::eq(
                object as *const AnyObject,
                table.as_ref() as *const NSTableView as *const AnyObject,
            )
        })
    }

    fn table_matches_preferences_devices_table(&self, table_view: &NSTableView) -> bool {
        self.preferences_devices_table()
            .is_some_and(|table| std::ptr::eq(table.as_ref(), table_view))
    }

    fn object_matches_preferences_field(&self, object: &AnyObject) -> bool {
        self.preferences_device_name_field().is_some_and(|field| {
            std::ptr::eq(
                object as *const AnyObject,
                field.as_ref() as *const NSTextField as *const AnyObject,
            )
        }) || self.preferences_history_limit_field().is_some_and(|field| {
            std::ptr::eq(
                object as *const AnyObject,
                field.as_ref() as *const NSTextField as *const AnyObject,
            )
        })
    }

    fn window_matches_panel(&self, window: &NSWindow) -> bool {
        self.panel()
            .is_some_and(|panel| std::ptr::eq(window, panel.as_ref()))
    }

    fn window_matches_preferences(&self, window: &NSWindow) -> bool {
        self.preferences_window()
            .is_some_and(|preferences| std::ptr::eq(window, preferences.as_ref()))
    }

    fn window_matches_about(&self, window: &NSWindow) -> bool {
        self.about_window()
            .is_some_and(|about| std::ptr::eq(window, about.as_ref()))
    }

    fn capture_frontmost_application(&self) {
        let bundle_id = NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .and_then(|app| app.bundleIdentifier())
            .map(|bundle| bundle.to_string())
            .filter(|bundle| !bundle.is_empty());
        *self.ivars().previous_frontmost_bundle_id.borrow_mut() = bundle_id;
    }

    fn trigger_immediate_paste(&self) {
        paste::trigger_immediate_paste(self.ivars().previous_frontmost_bundle_id.borrow().clone());
    }

    fn panel_visible(&self) -> bool {
        self.panel().is_some_and(|panel| panel.isVisible())
    }
}

pub(super) fn create_clipboard_backend() -> AppResult<Box<dyn ClipboardBackend>> {
    clipboard::create_clipboard_backend()
}

pub(crate) fn create_local_data_cipher(
    paths: &crate::core::paths::AppPaths,
) -> AppResult<crate::core::at_rest::LocalDataCipher> {
    keychain::create_local_data_cipher(paths)
}

pub fn run(controller: AppController) -> PlatformResult {
    let mtm = MainThreadMarker::new().ok_or("AppKit 必须在主线程中运行。")?;
    let app = NSApplication::sharedApplication(mtm);
    let delegate = AppDelegate::new(mtm, controller);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
    Ok(())
}
