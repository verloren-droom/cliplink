pub(super) use super::{
    ui::{
        FEEDBACK_HEIGHT, FOOTER_HEIGHT, HEADER_HEIGHT, OUTER_PADDING, PANEL_HEIGHT, PANEL_WIDTH,
        ROW_HEIGHT,
    },
    *,
};
pub(super) use objc2::sel;
pub(super) use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn};
pub(super) use objc2_foundation::{NSString, NSURL, ns_string};
pub(super) use std::collections::BTreeSet;

pub(super) use crate::constants::app::APP_NAME;
pub(super) use crate::core::model::ClipboardPayload;
pub(super) use crate::platform::macos::selection::{index_set_from_rows, selected_rows_from_table};
pub(super) use crate::platform::macos::widgets::{
    FooterShortcut, make_background_box, make_checkbox, make_footer_button, make_separator,
};

mod actions;
mod state;
mod window;

const FOOTER_BUTTON_HEIGHT: f64 = 22.0;
const FOOTER_BUTTON_X_OFFSET: f64 = 6.0;
const FOOTER_BUTTON_CLEAR_Y: f64 = 70.0;
const FOOTER_BUTTON_PREFERENCES_Y: f64 = 48.0;
const FOOTER_BUTTON_ABOUT_Y: f64 = 26.0;
const FOOTER_BUTTON_QUIT_Y: f64 = 4.0;
const FOOTER_BUTTON_HORIZONTAL_PADDING: f64 = 4.0;
const FEEDBACK_BAR_HEIGHT: f64 = 6.0;

pub(super) fn point_in_rect(point: NSPoint, rect: NSRect) -> bool {
    point.x >= rect.origin.x
        && point.x <= rect.origin.x + rect.size.width
        && point.y >= rect.origin.y
        && point.y <= rect.origin.y + rect.size.height
}
