use std::{thread, time::Duration};

use windows_sys::Win32::{
    Foundation::HWND,
    UI::{
        Input::KeyboardAndMouse::{KEYEVENTF_KEYUP, VK_CONTROL, keybd_event},
        WindowsAndMessaging::{
            GetForegroundWindow, IsIconic, SW_RESTORE, SetForegroundWindow, ShowWindow,
        },
    },
};

use crate::constants::timing::IMMEDIATE_PASTE_DELAY;

const VIRTUAL_KEY_V: u8 = b'V';

pub(super) fn capture_foreground_window(excluded: &[HWND]) -> Option<HWND> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() || excluded.contains(&hwnd) {
            None
        } else {
            Some(hwnd)
        }
    }
}

pub(super) fn trigger_immediate_paste(previous_foreground: Option<HWND>) {
    let previous_foreground = previous_foreground.map(|hwnd| hwnd as usize);
    let _ = thread::Builder::new()
        .name("cliplink-windows-paste".to_string())
        .spawn(move || unsafe {
            if let Some(hwnd) = previous_foreground {
                let hwnd = hwnd as HWND;
                if IsIconic(hwnd) != 0 {
                    ShowWindow(hwnd, SW_RESTORE);
                }
                let _ = SetForegroundWindow(hwnd);
            }

            thread::sleep(Duration::from_millis(
                IMMEDIATE_PASTE_DELAY.as_millis() as u64
            ));

            keybd_event(VK_CONTROL as u8, 0, 0, 0);
            keybd_event(VIRTUAL_KEY_V, 0, 0, 0);
            keybd_event(VIRTUAL_KEY_V, 0, KEYEVENTF_KEYUP, 0);
            keybd_event(VK_CONTROL as u8, 0, KEYEVENTF_KEYUP, 0);
        });
}
