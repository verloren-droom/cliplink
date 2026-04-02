use std::thread;

use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
use objc2_core_graphics::{
    CGEvent, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation,
};
use objc2_foundation::NSString;

use crate::constants::timing::IMMEDIATE_PASTE_DELAY;

/// ANSI virtual key code for the `V` key used to replay the standard paste shortcut.
const PASTE_KEY_CODE_V: u16 = 9;

pub(super) fn trigger_immediate_paste(previous_frontmost_bundle_id: Option<String>) {
    let _ = thread::Builder::new()
        .name("cliplink-paste".to_string())
        .spawn(move || {
            if let Some(bundle_id) = previous_frontmost_bundle_id.as_deref() {
                focus_application(bundle_id);
            }

            thread::sleep(IMMEDIATE_PASTE_DELAY);
            let _ = post_paste_shortcut();
        });
}

fn focus_application(bundle_id: &str) {
    let applications = NSRunningApplication::runningApplicationsWithBundleIdentifier(
        &NSString::from_str(bundle_id),
    );
    if let Some(application) = applications.iter().next() {
        let _ = application.activateWithOptions(NSApplicationActivationOptions::empty());
    }
}

fn post_paste_shortcut() -> bool {
    let Some(source) = CGEventSource::new(CGEventSourceStateID::CombinedSessionState) else {
        return false;
    };

    CGEventSource::set_local_events_suppression_interval(Some(source.as_ref()), 0.0);

    let Some(key_down) = CGEvent::new_keyboard_event(Some(source.as_ref()), PASTE_KEY_CODE_V, true)
    else {
        return false;
    };
    let Some(key_up) = CGEvent::new_keyboard_event(Some(source.as_ref()), PASTE_KEY_CODE_V, false)
    else {
        return false;
    };

    CGEvent::set_flags(Some(key_down.as_ref()), CGEventFlags::MaskCommand);
    CGEvent::set_flags(Some(key_up.as_ref()), CGEventFlags::MaskCommand);
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(key_down.as_ref()));
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(key_up.as_ref()));
    true
}
