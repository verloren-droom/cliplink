pub(super) use super::{
    ui::{PREFERENCES_DEVICE_ROW_HEIGHT, PREFERENCES_HEIGHT, PREFERENCES_WIDTH},
    *,
};
pub(super) use std::ptr::{NonNull, null_mut};

pub(super) use block2::RcBlock;
pub(super) use global_hotkey::hotkey::HotKey;
pub(super) use objc2::sel;
pub(super) use objc2_app_kit::NSAlert;
pub(super) use objc2_foundation::ns_string;

pub(super) use crate::{
    constants::limits::{MAX_DEVICE_NAME_CHARS, MAX_HISTORY_LIMIT},
    platform::macos::hotkey::{format_hotkey_for_display, hotkey_from_event},
    platform::macos::selection::{index_set_from_rows, selected_rows_from_table},
    platform::macos::widgets::{
        make_background_box, make_checkbox, make_field_label, make_input_field, make_menu_item,
        make_secondary_label,
    },
};

mod capture;
mod devices;
mod form;
mod window;

const HOTKEY_CAPTURE_HINT_IDLE: &str = "点击“录制”后直接按下新的组合键";
const HOTKEY_CAPTURE_HINT_ACTIVE: &str = "请按下新的快捷键，按 Esc 取消";
const HOTKEY_CAPTURE_BUTTON_IDLE: &str = "录制";
const HOTKEY_CAPTURE_BUTTON_ACTIVE: &str = "取消";

type DevicesPreferencesTabViews = (
    Retained<NSView>,
    Retained<NSTableView>,
    Retained<NSMenu>,
    Retained<NSMenuItem>,
    Retained<NSMenuItem>,
);
