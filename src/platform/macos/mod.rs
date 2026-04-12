#![deny(unsafe_op_in_unsafe_fn)]

use std::{
    cell::{OnceCell, RefCell},
    ptr::NonNull,
};

use block2::RcBlock;
use global_hotkey::{GlobalHotKeyManager, hotkey::HotKey};
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
    NSImageView, NSLineBreakMode, NSMenu, NSMenuDelegate, NSMenuItem, NSPanel, NSPopUpButton,
    NSPopUpMenuWindowLevel, NSScreen, NSScrollView, NSSearchField, NSSearchFieldDelegate,
    NSStatusBar, NSStatusItem, NSStatusItemBehavior, NSTabView, NSTabViewItem, NSTabViewType,
    NSTableCellView, NSTableColumn, NSTableColumnResizingOptions, NSTableView,
    NSTableViewColumnAutoresizingStyle, NSTableViewDataSource, NSTableViewDelegate,
    NSTableViewRowSizeStyle, NSTableViewSelectionHighlightStyle, NSTableViewStyle, NSTextField,
    NSTextFieldDelegate, NSTextView, NSVariableStatusItemLength, NSView, NSWindow,
    NSWindowCollectionBehavior, NSWindowDelegate, NSWindowStyleMask, NSWindowTitleVisibility,
    NSWorkspace,
};
use objc2_foundation::{
    MainThreadMarker, NSInteger, NSMutableIndexSet, NSNotFound, NSNotification, NSObject,
    NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSTimer, NSUInteger,
};
use uuid::Uuid;

use crate::{
    controller::{
        AppController, HistoryRow, HistoryScope, HistoryScopeOption, SettingsDeviceEntry,
        SettingsUpdate,
    },
    core::{clipboard::ClipboardBackend, error::AppResult},
    platform::PlatformResult,
};

mod about;
mod accessors;
mod actions;
mod autostart;
mod clipboard;
mod hotkey;
mod icons;
mod keychain;
mod panel;
mod paste;
mod preferences;
mod selection;
mod tray;
mod trust;
mod ui;
mod widgets;

use self::ui::{PREFERENCES_DEVICE_ROW_HEIGHT, ROW_HEIGHT};

type HotKeyMonitorBlock = RcBlock<dyn Fn(NonNull<NSEvent>) -> *mut NSEvent>;

struct AppDelegateIvars {
    controller: RefCell<AppController>,
    search_query: RefCell<String>,
    history_scope_key: RefCell<String>,
    history_scope_options: RefCell<Vec<HistoryScopeOption>>,
    filtered_rows: RefCell<Vec<HistoryRow>>,
    selected_row: RefCell<Option<usize>>,
    selected_history_ids: RefCell<Vec<Uuid>>,
    preferences_baseline: RefCell<Option<SettingsUpdate>>,
    preferences_hotkey_value: RefCell<String>,
    menu_history_ids: RefCell<Vec<Uuid>>,
    previous_frontmost_bundle_id: RefCell<Option<String>>,
    active_trust_prompt_id: RefCell<Option<Uuid>>,
    application_icon: OnceCell<Retained<NSImage>>,
    status_item: OnceCell<Retained<NSStatusItem>>,
    status_item_icon: OnceCell<Retained<NSImage>>,
    status_menu: OnceCell<Retained<NSMenu>>,
    history_context_menu: RefCell<Option<Retained<NSMenu>>>,
    history_context_separator_item: RefCell<Option<Retained<NSMenuItem>>>,
    history_context_pin_item: RefCell<Option<Retained<NSMenuItem>>>,
    history_context_open_folder_item: RefCell<Option<Retained<NSMenuItem>>>,
    history_context_delete_item: RefCell<Option<Retained<NSMenuItem>>>,
    panel: RefCell<Option<Retained<NSPanel>>>,
    about_window: RefCell<Option<Retained<NSWindow>>>,
    search_field: RefCell<Option<Retained<NSSearchField>>>,
    history_scope_button: RefCell<Option<Retained<NSPopUpButton>>>,
    table_view: RefCell<Option<Retained<NSTableView>>>,
    panel_status_label: RefCell<Option<Retained<NSTextField>>>,
    panel_progress_track: RefCell<Option<Retained<NSTextField>>>,
    panel_progress_fill: RefCell<Option<Retained<NSTextField>>>,
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
    preferences_selected_device_ids: RefCell<Vec<String>>,
    preferences_devices_context_menu: RefCell<Option<Retained<NSMenu>>>,
    preferences_devices_context_action_item: RefCell<Option<Retained<NSMenuItem>>>,
    preferences_devices_context_properties_item: RefCell<Option<Retained<NSMenuItem>>>,
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

            self.install_application_icon();
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

    unsafe impl NSMenuDelegate for AppDelegate {
        #[unsafe(method(menuWillOpen:))]
        fn menu_will_open(&self, menu: &NSMenu) {
            if self.menu_matches_history_context_menu(menu) {
                self.refresh_history_context_menu_state();
            } else if self.menu_matches_preferences_devices_context_menu(menu) {
                self.refresh_preferences_device_actions();
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
            } else if (command_selector == sel!(deleteToBeginningOfLine:)
                || command_selector == sel!(deleteToBeginningOfParagraph:))
                && NSApplication::sharedApplication(self.mtm())
                    .currentEvent()
                    .is_some_and(|event| {
                        event
                            .modifierFlags()
                            .contains(NSEventModifierFlags::Command)
                    })
            {
                self.perform_clear_history_action();
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
            self.perform_status_item_action();
        }

        #[unsafe(method(tick:))]
        fn tick_action(&self, _timer: &NSTimer) {
            self.perform_tick(_timer);
        }

        #[unsafe(method(activateSelected:))]
        fn activate_selected_action(&self, sender: Option<&AnyObject>) {
            self.perform_activate_selected_action(sender);
        }

        #[unsafe(method(deleteHistoryItem:))]
        fn delete_history_item_action(&self, sender: Option<&AnyObject>) {
            self.perform_delete_history_item_action(sender);
        }

        #[unsafe(method(toggleHistoryItemPin:))]
        fn toggle_history_item_pin_action(&self, sender: Option<&AnyObject>) {
            self.perform_toggle_history_item_pin_action(sender);
        }

        #[unsafe(method(openHistoryItemParentFolders:))]
        fn open_history_item_parent_folders_action(&self, sender: Option<&AnyObject>) {
            self.perform_open_history_item_parent_folders_action(sender);
        }

        #[unsafe(method(clearHistory:))]
        fn clear_history_action(&self, _sender: Option<&AnyObject>) {
            self.perform_clear_history_action();
        }

        #[unsafe(method(openPreferences:))]
        fn open_preferences_action(&self, _sender: Option<&AnyObject>) {
            self.perform_open_preferences_action();
        }

        #[unsafe(method(openAbout:))]
        fn open_about_action(&self, _sender: Option<&AnyObject>) {
            self.perform_open_about_action();
        }

        #[unsafe(method(historyScopeChanged:))]
        fn history_scope_changed_action(&self, _sender: Option<&AnyObject>) {
            self.perform_history_scope_changed_action();
        }

        #[unsafe(method(closeAbout:))]
        fn close_about_action(&self, _sender: Option<&AnyObject>) {
            self.perform_close_about_action();
        }

        #[unsafe(method(activateMenuHistory:))]
        fn activate_menu_history_action(&self, sender: Option<&AnyObject>) {
            self.perform_activate_menu_history_action(sender);
        }

        #[unsafe(method(savePreferences:))]
        fn save_preferences_action(&self, _sender: Option<&AnyObject>) {
            self.perform_save_preferences_action();
        }

        #[unsafe(method(closePreferences:))]
        fn close_preferences_action(&self, _sender: Option<&AnyObject>) {
            self.perform_close_preferences_action();
        }

        #[unsafe(method(preferencesChanged:))]
        fn preferences_changed_action(&self, _sender: Option<&AnyObject>) {
            self.perform_preferences_changed_action();
        }

        #[unsafe(method(toggleHotkeyCapture:))]
        fn toggle_hotkey_capture_action(&self, _sender: Option<&AnyObject>) {
            self.perform_toggle_hotkey_capture_action();
        }

        #[unsafe(method(trustSelectedDevice:))]
        fn trust_selected_device_action(&self, _sender: Option<&AnyObject>) {
            self.perform_trust_selected_device_action();
        }

        #[unsafe(method(revokeSelectedDevice:))]
        fn revoke_selected_device_action(&self, _sender: Option<&AnyObject>) {
            self.perform_revoke_selected_device_action();
        }

        #[unsafe(method(showSelectedDeviceProperties:))]
        fn show_selected_device_properties_action(&self, _sender: Option<&AnyObject>) {
            self.perform_show_selected_device_properties_action();
        }

        #[unsafe(method(quitApp:))]
        fn quit_app_action(&self, _sender: Option<&AnyObject>) {
            self.perform_quit_app_action();
        }
    }
);

impl AppDelegate {
    fn with_controller<R>(&self, f: impl FnOnce(&AppController) -> R) -> R {
        let controller = self.ivars().controller.borrow();
        f(&controller)
    }

    fn with_controller_mut<R>(&self, f: impl FnOnce(&mut AppController) -> R) -> R {
        let mut controller = self.ivars().controller.borrow_mut();
        f(&mut controller)
    }

    fn report_controller_status(&self, message: impl Into<String>) {
        let message = message.into();
        self.with_controller_mut(|controller| controller.report_status(message));
    }

    fn new(mtm: MainThreadMarker, controller: AppController) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(AppDelegateIvars {
            controller: RefCell::new(controller),
            search_query: RefCell::new(String::new()),
            history_scope_key: RefCell::new(HistoryScope::All.key()),
            history_scope_options: RefCell::new(Vec::new()),
            filtered_rows: RefCell::new(Vec::new()),
            selected_row: RefCell::new(None),
            selected_history_ids: RefCell::new(Vec::new()),
            preferences_baseline: RefCell::new(None),
            preferences_hotkey_value: RefCell::new(String::new()),
            menu_history_ids: RefCell::new(Vec::new()),
            previous_frontmost_bundle_id: RefCell::new(None),
            active_trust_prompt_id: RefCell::new(None),
            application_icon: OnceCell::new(),
            status_item: OnceCell::new(),
            status_item_icon: OnceCell::new(),
            status_menu: OnceCell::new(),
            history_context_menu: RefCell::new(None),
            history_context_separator_item: RefCell::new(None),
            history_context_pin_item: RefCell::new(None),
            history_context_open_folder_item: RefCell::new(None),
            history_context_delete_item: RefCell::new(None),
            panel: RefCell::new(None),
            about_window: RefCell::new(None),
            search_field: RefCell::new(None),
            history_scope_button: RefCell::new(None),
            table_view: RefCell::new(None),
            panel_status_label: RefCell::new(None),
            panel_progress_track: RefCell::new(None),
            panel_progress_fill: RefCell::new(None),
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
            preferences_selected_device_ids: RefCell::new(Vec::new()),
            preferences_devices_context_menu: RefCell::new(None),
            preferences_devices_context_action_item: RefCell::new(None),
            preferences_devices_context_properties_item: RefCell::new(None),
            hotkey_manager: RefCell::new(None),
            hotkey: RefCell::new(None),
            hotkey_capture_monitor: RefCell::new(None),
            hotkey_capture_block: RefCell::new(None),
        });

        unsafe { msg_send![super(this), init] }
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

pub(super) fn run(controller: AppController) -> PlatformResult {
    let mtm = MainThreadMarker::new().ok_or("AppKit 必须在主线程中运行。")?;
    let app = NSApplication::sharedApplication(mtm);
    let delegate = AppDelegate::new(mtm, controller);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
    Ok(())
}
