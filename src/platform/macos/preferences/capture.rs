use super::*;

impl AppDelegate {
    pub(in crate::platform::macos) fn toggle_hotkey_capture(&self) {
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

    pub(in crate::platform::macos) fn stop_hotkey_capture(&self, cancelled: bool) {
        if let Some(monitor) = self.ivars().hotkey_capture_monitor.borrow_mut().take() {
            unsafe { NSEvent::removeMonitor(&monitor) };
        }
        self.ivars().hotkey_capture_block.borrow_mut().take();
        self.refresh_hotkey_capture_ui(false);
        if cancelled {
            self.set_preferences_status("已取消快捷键录制。");
        }
    }

    pub(in crate::platform::macos) fn hotkey_capture_active(&self) -> bool {
        self.ivars().hotkey_capture_monitor.borrow().is_some()
    }

    pub(in crate::platform::macos) fn refresh_hotkey_capture_ui(&self, active: bool) {
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
}
