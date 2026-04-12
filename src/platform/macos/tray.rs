use super::{ui::TRAY_HISTORY_LIMIT, *};
use std::str::FromStr;

use objc2_foundation::ns_string;

use crate::{
    constants::{app::APP_NAME, timing::MACOS_UI_TICK_INTERVAL_SECONDS},
    platform::macos::widgets::{make_menu_item, truncate_menu_title},
};

impl AppDelegate {
    pub(super) fn install_application_icon(&self) {
        let app = NSApplication::sharedApplication(self.mtm());
        unsafe {
            app.setApplicationIconImage(Some(self.application_icon_image()));
        }
    }

    pub(super) fn install_status_item(&self, mtm: MainThreadMarker) {
        let status_bar = NSStatusBar::systemStatusBar();
        let status_item = status_bar.statusItemWithLength(NSVariableStatusItemLength);
        status_item.setBehavior(NSStatusItemBehavior::RemovalAllowed);

        if let Some(button) = status_item.button(mtm) {
            button.setTitle(ns_string!(""));
            button.setImage(Some(self.status_item_icon_image()));
            button.setImageScaling(NSImageScaling::ScaleProportionallyDown);
            button.sendActionOn(NSEventMask::LeftMouseUp);
            unsafe {
                button.setTarget(Some(self));
                button.setAction(Some(sel!(statusItemAction:)));
            }
        }

        let _ = self.ivars().status_item.set(status_item);
    }

    pub(super) fn install_status_menu(&self, mtm: MainThreadMarker) {
        let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(APP_NAME));
        self.rebuild_status_menu(&menu);
        let _ = self.ivars().status_menu.set(menu);
    }

    pub(super) fn ensure_status_menu(&self) {
        if self.status_menu().is_none() {
            self.install_status_menu(self.mtm());
        }
    }

    pub(super) fn rebuild_status_menu(&self, menu: &NSMenu) {
        menu.removeAllItems();

        let entries = self
            .ivars()
            .controller
            .borrow()
            .recent_menu_entries(TRAY_HISTORY_LIMIT);
        self.ivars()
            .menu_history_ids
            .replace(entries.iter().map(|(id, _)| *id).collect());

        if entries.is_empty() {
            let empty_item = make_menu_item(self.mtm(), "暂无剪切板历史", None, None, "");
            empty_item.setEnabled(false);
            menu.addItem(&empty_item);
        } else {
            for (index, (_id, summary)) in entries.iter().enumerate() {
                let item = make_menu_item(
                    self.mtm(),
                    &truncate_menu_title(summary),
                    Some(self),
                    Some(sel!(activateMenuHistory:)),
                    "",
                );
                item.setTag(index as NSInteger);
                item.setToolTip(Some(&NSString::from_str(summary)));
                menu.addItem(&item);
            }
        }

        menu.addItem(&NSMenuItem::separatorItem(self.mtm()));
        menu.addItem(&make_menu_item(
            self.mtm(),
            "偏好设置...",
            Some(self),
            Some(sel!(openPreferences:)),
            ",",
        ));
        menu.addItem(&make_menu_item(
            self.mtm(),
            "关于",
            Some(self),
            Some(sel!(openAbout:)),
            "",
        ));
        menu.addItem(&NSMenuItem::separatorItem(self.mtm()));
        menu.addItem(&make_menu_item(
            self.mtm(),
            "退出",
            Some(self),
            Some(sel!(quitApp:)),
            "q",
        ));
    }

    pub(super) fn install_hotkey(&self) {
        let _ = self.refresh_hotkey_binding();
    }

    pub(super) fn refresh_hotkey_binding(&self) -> Result<(), String> {
        if let Some(existing) = self.ivars().hotkey.borrow_mut().take() {
            if let Some(manager) = self.ivars().hotkey_manager.borrow().as_ref() {
                let _ = manager.unregister(existing);
            }
        }

        let hotkey = self.ivars().controller.borrow().hotkey().to_string();

        if self.ivars().hotkey_manager.borrow().is_none() {
            *self.ivars().hotkey_manager.borrow_mut() =
                Some(GlobalHotKeyManager::new().map_err(|error| error.to_string())?);
        }

        let parsed = HotKey::from_str(&hotkey).map_err(|error| error.to_string())?;
        if let Some(manager) = self.ivars().hotkey_manager.borrow().as_ref() {
            manager
                .register(parsed)
                .map_err(|error| error.to_string())?;
        }
        *self.ivars().hotkey.borrow_mut() = Some(parsed);
        Ok(())
    }

    pub(super) fn install_timer(&self) {
        unsafe {
            let _ = NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                MACOS_UI_TICK_INTERVAL_SECONDS,
                self,
                sel!(tick:),
                None,
                true,
            );
        }
    }

    pub(super) fn handle_status_item_click(&self) {
        let app = NSApplication::sharedApplication(self.mtm());
        let event_type = app.currentEvent().map(|event| event.r#type());
        if matches!(event_type, Some(kind) if kind == NSEventType::LeftMouseUp) {
            self.show_status_menu();
        }
    }

    pub(super) fn show_status_menu(&self) {
        self.hide_panel(false);
        self.capture_frontmost_application();
        self.ensure_status_menu();
        let (Some(status_item), Some(menu)) = (self.status_item(), self.status_menu()) else {
            return;
        };
        self.rebuild_status_menu(menu);
        #[allow(deprecated)]
        status_item.popUpStatusItemMenu(menu);
    }
}
