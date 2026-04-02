use objc2::{DefinedClass, rc::Retained, runtime::AnyObject};
use objc2_app_kit::{
    NSButton, NSMenu, NSMenuItem, NSPanel, NSPopUpButton, NSSearchField, NSStatusItem, NSTableView,
    NSTextField, NSWindow, NSWorkspace,
};

use super::{AppDelegate, paste};

impl AppDelegate {
    pub(super) fn panel(&self) -> Option<Retained<NSPanel>> {
        self.ivars().panel.borrow().clone()
    }

    pub(super) fn search_field(&self) -> Option<Retained<NSSearchField>> {
        self.ivars().search_field.borrow().clone()
    }

    pub(super) fn history_scope_button(&self) -> Option<Retained<NSPopUpButton>> {
        self.ivars().history_scope_button.borrow().clone()
    }

    pub(super) fn about_window(&self) -> Option<Retained<NSWindow>> {
        self.ivars().about_window.borrow().clone()
    }

    pub(super) fn table_view(&self) -> Option<Retained<NSTableView>> {
        self.ivars().table_view.borrow().clone()
    }

    pub(super) fn panel_status_label(&self) -> Option<Retained<NSTextField>> {
        self.ivars().panel_status_label.borrow().clone()
    }

    pub(super) fn panel_progress_track(&self) -> Option<Retained<NSTextField>> {
        self.ivars().panel_progress_track.borrow().clone()
    }

    pub(super) fn panel_progress_fill(&self) -> Option<Retained<NSTextField>> {
        self.ivars().panel_progress_fill.borrow().clone()
    }

    pub(super) fn status_item(&self) -> Option<&NSStatusItem> {
        self.ivars().status_item.get().map(|item| &**item)
    }

    pub(super) fn status_menu(&self) -> Option<&NSMenu> {
        self.ivars().status_menu.get().map(|menu| &**menu)
    }

    pub(super) fn history_context_menu(&self) -> Option<Retained<NSMenu>> {
        self.ivars().history_context_menu.borrow().clone()
    }

    pub(super) fn history_context_separator_item(&self) -> Option<Retained<NSMenuItem>> {
        self.ivars().history_context_separator_item.borrow().clone()
    }

    pub(super) fn history_context_pin_item(&self) -> Option<Retained<NSMenuItem>> {
        self.ivars().history_context_pin_item.borrow().clone()
    }

    pub(super) fn history_context_open_folder_item(&self) -> Option<Retained<NSMenuItem>> {
        self.ivars()
            .history_context_open_folder_item
            .borrow()
            .clone()
    }

    pub(super) fn history_context_delete_item(&self) -> Option<Retained<NSMenuItem>> {
        self.ivars().history_context_delete_item.borrow().clone()
    }

    pub(super) fn preferences_window(&self) -> Option<Retained<NSWindow>> {
        self.ivars().preferences_window.borrow().clone()
    }

    pub(super) fn preferences_status_label(&self) -> Option<Retained<NSTextField>> {
        self.ivars().preferences_status_label.borrow().clone()
    }

    pub(super) fn preferences_save_button(&self) -> Option<Retained<NSButton>> {
        self.ivars().preferences_save_button.borrow().clone()
    }

    pub(super) fn preferences_device_name_field(&self) -> Option<Retained<NSTextField>> {
        self.ivars().preferences_device_name_field.borrow().clone()
    }

    pub(super) fn preferences_history_limit_field(&self) -> Option<Retained<NSTextField>> {
        self.ivars()
            .preferences_history_limit_field
            .borrow()
            .clone()
    }

    pub(super) fn preferences_hotkey_field(&self) -> Option<Retained<NSTextField>> {
        self.ivars().preferences_hotkey_field.borrow().clone()
    }

    pub(super) fn preferences_hotkey_record_button(&self) -> Option<Retained<NSButton>> {
        self.ivars()
            .preferences_hotkey_record_button
            .borrow()
            .clone()
    }

    pub(super) fn preferences_hotkey_preview_label(&self) -> Option<Retained<NSTextField>> {
        self.ivars()
            .preferences_hotkey_preview_label
            .borrow()
            .clone()
    }

    pub(super) fn preferences_launch_at_login_checkbox(&self) -> Option<Retained<NSButton>> {
        self.ivars()
            .preferences_launch_at_login_checkbox
            .borrow()
            .clone()
    }

    pub(super) fn preferences_share_checkbox(&self) -> Option<Retained<NSButton>> {
        self.ivars().preferences_share_checkbox.borrow().clone()
    }

    pub(super) fn preferences_prefer_remote_paste_checkbox(&self) -> Option<Retained<NSButton>> {
        self.ivars()
            .preferences_prefer_remote_paste_checkbox
            .borrow()
            .clone()
    }

    pub(super) fn preferences_discovery_checkbox(&self) -> Option<Retained<NSButton>> {
        self.ivars().preferences_discovery_checkbox.borrow().clone()
    }

    pub(super) fn preferences_devices_table(&self) -> Option<Retained<NSTableView>> {
        self.ivars().preferences_devices_table.borrow().clone()
    }

    pub(super) fn preferences_devices_context_menu(&self) -> Option<Retained<NSMenu>> {
        self.ivars()
            .preferences_devices_context_menu
            .borrow()
            .clone()
    }

    pub(super) fn preferences_devices_context_action_item(&self) -> Option<Retained<NSMenuItem>> {
        self.ivars()
            .preferences_devices_context_action_item
            .borrow()
            .clone()
    }

    pub(super) fn preferences_devices_context_properties_item(
        &self,
    ) -> Option<Retained<NSMenuItem>> {
        self.ivars()
            .preferences_devices_context_properties_item
            .borrow()
            .clone()
    }

    pub(super) fn object_matches_search_field(&self, object: &AnyObject) -> bool {
        self.search_field().is_some_and(|field| {
            std::ptr::eq(
                object as *const AnyObject,
                field.as_ref() as *const NSSearchField as *const AnyObject,
            )
        })
    }

    pub(super) fn object_matches_history_table(&self, object: &AnyObject) -> bool {
        self.table_view().is_some_and(|table| {
            std::ptr::eq(
                object as *const AnyObject,
                table.as_ref() as *const NSTableView as *const AnyObject,
            )
        })
    }

    pub(super) fn object_matches_preferences_devices_table(&self, object: &AnyObject) -> bool {
        self.preferences_devices_table().is_some_and(|table| {
            std::ptr::eq(
                object as *const AnyObject,
                table.as_ref() as *const NSTableView as *const AnyObject,
            )
        })
    }

    pub(super) fn table_matches_preferences_devices_table(&self, table_view: &NSTableView) -> bool {
        self.preferences_devices_table()
            .is_some_and(|table| std::ptr::eq(table.as_ref(), table_view))
    }

    pub(super) fn menu_matches_history_context_menu(&self, menu: &NSMenu) -> bool {
        self.history_context_menu()
            .is_some_and(|current| std::ptr::eq(menu, current.as_ref()))
    }

    pub(super) fn menu_matches_preferences_devices_context_menu(&self, menu: &NSMenu) -> bool {
        self.preferences_devices_context_menu()
            .is_some_and(|current| std::ptr::eq(menu, current.as_ref()))
    }

    pub(super) fn object_matches_preferences_field(&self, object: &AnyObject) -> bool {
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

    pub(super) fn window_matches_panel(&self, window: &NSWindow) -> bool {
        self.panel()
            .is_some_and(|panel| std::ptr::eq(window, panel.as_ref()))
    }

    pub(super) fn window_matches_preferences(&self, window: &NSWindow) -> bool {
        self.preferences_window()
            .is_some_and(|preferences| std::ptr::eq(window, preferences.as_ref()))
    }

    pub(super) fn window_matches_about(&self, window: &NSWindow) -> bool {
        self.about_window()
            .is_some_and(|about| std::ptr::eq(window, about.as_ref()))
    }

    pub(super) fn capture_frontmost_application(&self) {
        let bundle_id = NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .and_then(|app| app.bundleIdentifier())
            .map(|bundle| bundle.to_string())
            .filter(|bundle| !bundle.is_empty());
        *self.ivars().previous_frontmost_bundle_id.borrow_mut() = bundle_id;
    }

    pub(super) fn trigger_immediate_paste(&self) {
        paste::trigger_immediate_paste(self.ivars().previous_frontmost_bundle_id.borrow().clone());
    }

    pub(super) fn panel_visible(&self) -> bool {
        self.panel().is_some_and(|panel| panel.isVisible())
    }
}
