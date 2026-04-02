use super::{
    ui::{PREFERENCES_DEVICE_ROW_HEIGHT, ROW_HEIGHT},
    *,
};
use crate::controller::{DeviceStatusKind, HistoryRow, SettingsDeviceEntry};
use objc2_quartz_core::CALayer;

const HISTORY_BADGE_HEIGHT: f64 = 20.0;
const HISTORY_BADGE_REMOTE_MIN_WIDTH: f64 = 52.0;
const HISTORY_BADGE_LOCAL_MIN_WIDTH: f64 = 48.0;
const HISTORY_BADGE_MAX_WIDTH: f64 = 108.0;
const HISTORY_ROW_LEADING_X: f64 = 10.0;
const HISTORY_ROW_SCROLLBAR_SAFE_INSET: f64 = 52.0;
const HISTORY_ROW_TITLE_BADGE_GAP: f64 = 14.0;
const HISTORY_ROW_TITLE_HEIGHT: f64 = 18.0;
const HISTORY_ROW_METADATA_HEIGHT: f64 = 14.0;
const HISTORY_ROW_LABEL_GAP: f64 = 4.0;
const HISTORY_ROW_BOTTOM_INSET: f64 = 9.0;
const PREFERENCES_DEVICE_ROW_TOP_INSET: f64 = 8.0;
const PREFERENCES_DEVICE_ROW_BOTTOM_INSET: f64 = 8.0;
const PREFERENCES_DEVICE_ROW_TITLE_HEIGHT: f64 = 20.0;
const PREFERENCES_DEVICE_ROW_DETAIL_HEIGHT: f64 = 16.0;
const PREFERENCES_DEVICE_ROW_LABEL_GAP: f64 = 4.0;
const PREFERENCES_DEVICE_STATUS_DOT_WIDTH: f64 = 14.0;
const PREFERENCES_DEVICE_TRAILING_GAP: f64 = 12.0;
const PREFERENCES_DEVICE_TRAILING_SAFE_INSET: f64 = 28.0;
const FOOTER_BUTTON_TITLE_INSET: f64 = 12.0;
const FOOTER_BUTTON_SHORTCUT_WIDTH: f64 = 56.0;

#[derive(Clone, Copy)]
pub(super) struct FooterShortcut {
    pub display: &'static str,
    pub key_equivalent: &'static str,
    pub modifier_mask: NSEventModifierFlags,
}

pub(super) fn make_plain_button(
    mtm: MainThreadMarker,
    title: &str,
    target: &AnyObject,
    action: Sel,
    frame: NSRect,
) -> Retained<NSButton> {
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str(title),
            Some(target),
            Some(action),
            mtm,
        )
    };
    button.setFrame(frame);
    button.setBordered(false);
    button.setTransparent(false);
    button.setAlignment(objc2_app_kit::NSTextAlignment::Left);
    button
}

pub(super) fn make_footer_button(
    mtm: MainThreadMarker,
    title: &str,
    target: &AnyObject,
    action: Sel,
    frame: NSRect,
    shortcut: Option<FooterShortcut>,
) -> Retained<NSView> {
    let container = NSView::initWithFrame(NSView::alloc(mtm), frame);

    let title_label = NSTextField::labelWithString(&NSString::from_str(title), mtm);
    title_label.setFrame(NSRect::new(
        NSPoint::new(FOOTER_BUTTON_TITLE_INSET, 1.0),
        NSSize::new(
            (frame.size.width - FOOTER_BUTTON_TITLE_INSET * 2.0 - FOOTER_BUTTON_SHORTCUT_WIDTH)
                .max(120.0),
            frame.size.height - 2.0,
        ),
    ));
    title_label.setTextColor(Some(&NSColor::labelColor()));

    container.addSubview(&title_label);

    if let Some(shortcut) = shortcut {
        let shortcut_label =
            NSTextField::labelWithString(&NSString::from_str(shortcut.display), mtm);
        shortcut_label.setFrame(NSRect::new(
            NSPoint::new(frame.size.width - FOOTER_BUTTON_SHORTCUT_WIDTH - 8.0, 1.0),
            NSSize::new(FOOTER_BUTTON_SHORTCUT_WIDTH, frame.size.height - 2.0),
        ));
        shortcut_label.setAlignment(objc2_app_kit::NSTextAlignment::Right);
        shortcut_label.setTextColor(Some(&NSColor::secondaryLabelColor()));
        container.addSubview(&shortcut_label);
    }

    let button = make_plain_button(
        mtm,
        "",
        target,
        action,
        NSRect::new(NSPoint::new(0.0, 0.0), frame.size),
    );
    button.setKeyEquivalent(&NSString::from_str(
        shortcut.map(|value| value.key_equivalent).unwrap_or(""),
    ));
    button.setKeyEquivalentModifierMask(
        shortcut
            .map(|value| value.modifier_mask)
            .unwrap_or(NSEventModifierFlags(0)),
    );
    container.addSubview(&button);

    container
}

pub(super) fn make_menu_item(
    mtm: MainThreadMarker,
    title: &str,
    target: Option<&AnyObject>,
    action: Option<Sel>,
    key_equivalent: &str,
) -> Retained<NSMenuItem> {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            action,
            &NSString::from_str(key_equivalent),
        )
    };
    unsafe { item.setTarget(target) };
    item
}

pub(super) fn make_field_label(
    mtm: MainThreadMarker,
    text: &str,
    frame: NSRect,
) -> Retained<NSTextField> {
    let label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    label.setFrame(frame);
    label.setTextColor(Some(&NSColor::labelColor()));
    label
}

pub(super) fn make_secondary_label(
    mtm: MainThreadMarker,
    text: &str,
    frame: NSRect,
) -> Retained<NSTextField> {
    let label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    label.setFrame(frame);
    label.setTextColor(Some(&NSColor::secondaryLabelColor()));
    label
}

pub(super) fn make_history_primary_label(
    mtm: MainThreadMarker,
    text: &str,
    width: f64,
) -> Retained<NSTextField> {
    let label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    label.setFrame(NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(width.max(120.0), HISTORY_ROW_TITLE_HEIGHT),
    ));
    label.setFont(Some(&NSFont::systemFontOfSize(NSFont::systemFontSize())));
    label.setTextColor(Some(&NSColor::labelColor()));
    label.setUsesSingleLineMode(true);
    label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    label.setAllowsDefaultTighteningForTruncation(true);
    label.setMaximumNumberOfLines(1);
    if let Some(cell) = label.cell() {
        cell.setWraps(false);
        cell.setScrollable(false);
        cell.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        cell.setTruncatesLastVisibleLine(true);
    }
    label
}

fn make_history_metadata_label(
    mtm: MainThreadMarker,
    text: &str,
    width: f64,
) -> Retained<NSTextField> {
    let label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    label.setFrame(NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(width.max(120.0), HISTORY_ROW_METADATA_HEIGHT),
    ));
    label.setFont(Some(
        &NSFont::labelFontOfSize(NSFont::smallSystemFontSize()),
    ));
    label.setTextColor(Some(&NSColor::secondaryLabelColor()));
    label.setUsesSingleLineMode(true);
    label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    label.setAllowsDefaultTighteningForTruncation(true);
    label.setMaximumNumberOfLines(1);
    if let Some(cell) = label.cell() {
        cell.setWraps(false);
        cell.setScrollable(false);
        cell.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        cell.setTruncatesLastVisibleLine(true);
    }
    label
}

fn history_kind_text(row: &HistoryRow) -> &'static str {
    match row.kind.as_str() {
        "files" => "文件",
        _ => "文本",
    }
}

fn history_metadata_text(row: &HistoryRow) -> String {
    let mut parts = vec![history_kind_text(row).to_string()];
    if row.is_pinned {
        parts.push("已锁定".to_string());
    }
    parts.join(" · ")
}

fn history_primary_text(row: &HistoryRow) -> String {
    let summary = row.summary_text.trim();
    if row.kind != "files" {
        return summary.to_string();
    }

    if let Some(rest) = summary.strip_prefix("文件 · ") {
        return rest.trim().to_string();
    }

    if let Some((prefix, rest)) = summary.split_once(" · ") {
        if let Some(count) = prefix.trim().strip_suffix("个文件") {
            return format!("{} 项 · {}", count.trim(), rest.trim());
        }
    }

    summary.to_string()
}

pub(super) fn make_history_badge(
    mtm: MainThreadMarker,
    text: &str,
    is_remote: bool,
) -> Retained<NSView> {
    let width = history_badge_width(text, is_remote);
    let tint = if is_remote {
        NSColor::systemOrangeColor()
    } else {
        NSColor::secondaryLabelColor()
    };
    make_history_badge_view(mtm, text, width, &tint)
}

fn make_history_badge_view(
    mtm: MainThreadMarker,
    text: &str,
    width: f64,
    tint: &NSColor,
) -> Retained<NSView> {
    let badge = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(width, HISTORY_BADGE_HEIGHT),
        ),
    );
    let background = make_background_box(
        mtm,
        NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(width, HISTORY_BADGE_HEIGHT),
        ),
    );
    background.setBackgroundColor(Some(&tint.colorWithAlphaComponent(0.12)));

    let label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    label.setAlignment(objc2_app_kit::NSTextAlignment::Center);
    label.setFont(Some(
        &NSFont::labelFontOfSize(NSFont::smallSystemFontSize()),
    ));
    label.setTextColor(Some(tint));
    label.setUsesSingleLineMode(true);
    label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    label.setMaximumNumberOfLines(1);
    if let Some(cell) = label.cell() {
        cell.setWraps(false);
        cell.setScrollable(false);
        cell.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        cell.setTruncatesLastVisibleLine(true);
    }
    label.sizeToFit();
    let label_height = label.frame().size.height.max(12.0);
    let label_y = ((HISTORY_BADGE_HEIGHT - label_height) / 2.0)
        .round()
        .max(0.0);
    label.setFrame(NSRect::new(
        NSPoint::new(0.0, label_y.max(0.0)),
        NSSize::new(width, label_height),
    ));

    badge.addSubview(&background);
    badge.addSubview(&label);
    badge
}

fn history_badge_width(text: &str, is_remote: bool) -> f64 {
    let unit_width = text
        .chars()
        .map(|ch| if ch.is_ascii() { 1.0 } else { 1.7 })
        .sum::<f64>();
    (unit_width * 10.0 + 18.0).clamp(
        if is_remote {
            HISTORY_BADGE_REMOTE_MIN_WIDTH
        } else {
            HISTORY_BADGE_LOCAL_MIN_WIDTH
        },
        HISTORY_BADGE_MAX_WIDTH,
    )
}

#[derive(Clone, Copy)]
pub(super) struct HistoryRowAccessoryLayout {
    pub source_badge_origin: NSPoint,
}

pub(super) fn history_row_accessory_layout(
    row: &HistoryRow,
    total_width: f64,
) -> HistoryRowAccessoryLayout {
    let content_width = total_width.max(240.0);
    let badge_width = history_badge_width(&row.source_badge, row.is_remote);
    let trailing_edge = content_width - HISTORY_ROW_SCROLLBAR_SAFE_INSET;
    let source_badge_x = (trailing_edge - badge_width).max(HISTORY_ROW_LEADING_X + 120.0);
    let badge_y = ((ROW_HEIGHT - HISTORY_BADGE_HEIGHT) / 2.0).floor().max(6.0);

    HistoryRowAccessoryLayout {
        source_badge_origin: NSPoint::new(source_badge_x, badge_y),
    }
}

fn history_row_horizontal_spacing() -> f64 {
    NSFont::smallSystemFontSize().ceil().max(8.0)
}

pub(super) fn make_history_row_view(
    mtm: MainThreadMarker,
    row: &HistoryRow,
    total_width: f64,
    _is_selected: bool,
    _target: &AnyObject,
    _row_index: usize,
) -> Retained<NSTableCellView> {
    let content_width = total_width.max(240.0);
    let row_view = NSTableCellView::initWithFrame(
        NSTableCellView::alloc(mtm),
        NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(content_width, ROW_HEIGHT),
        ),
    );

    let row_spacing = history_row_horizontal_spacing();
    let accessory_layout = history_row_accessory_layout(row, content_width);

    let source_badge = {
        let badge = make_history_badge(mtm, &row.source_badge, row.is_remote);
        badge.setFrameOrigin(accessory_layout.source_badge_origin);
        if !row.source_tooltip.trim().is_empty() {
            badge.setToolTip(Some(&NSString::from_str(&row.source_tooltip)));
        }
        badge
    };

    let title_left_edge = HISTORY_ROW_LEADING_X + row_spacing * 0.25;
    let title_right_edge = source_badge.frame().origin.x - HISTORY_ROW_TITLE_BADGE_GAP;
    let text_width = (title_right_edge - title_left_edge).max(120.0);
    let title_container = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(
            NSPoint::new(title_left_edge.max(HISTORY_ROW_LEADING_X), 0.0),
            NSSize::new(text_width, ROW_HEIGHT),
        ),
    );

    let title = make_history_primary_label(mtm, &history_primary_text(row), text_width);
    let title_y = HISTORY_ROW_BOTTOM_INSET + HISTORY_ROW_METADATA_HEIGHT + HISTORY_ROW_LABEL_GAP;
    title.setFrame(NSRect::new(
        NSPoint::new(0.0, title_y),
        NSSize::new(text_width, HISTORY_ROW_TITLE_HEIGHT),
    ));
    title.setAlignment(objc2_app_kit::NSTextAlignment::Left);

    let metadata = make_history_metadata_label(mtm, &history_metadata_text(row), text_width);
    metadata.setFrame(NSRect::new(
        NSPoint::new(0.0, HISTORY_ROW_BOTTOM_INSET),
        NSSize::new(text_width, HISTORY_ROW_METADATA_HEIGHT),
    ));

    if !row.detail_tooltip.trim().is_empty() {
        let tooltip = NSString::from_str(&row.detail_tooltip);
        title.setToolTip(Some(&tooltip));
        metadata.setToolTip(Some(&tooltip));
        title_container.setToolTip(Some(&tooltip));
    }
    title_container.addSubview(&title);
    title_container.addSubview(&metadata);

    row_view.addSubview(&title_container);
    row_view.addSubview(&source_badge);
    unsafe {
        row_view.setTextField(Some(&title));
    }
    row_view
}

pub(super) fn make_preferences_device_row_view(
    mtm: MainThreadMarker,
    row: &SettingsDeviceEntry,
    total_width: f64,
) -> Retained<NSTableCellView> {
    let content_width = total_width.max(240.0);
    let row_view = NSTableCellView::initWithFrame(
        NSTableCellView::alloc(mtm),
        NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(content_width, PREFERENCES_DEVICE_ROW_HEIGHT),
        ),
    );

    let inset = 12.0;
    let trailing_width = PREFERENCES_DEVICE_TRAILING_SAFE_INSET
        + PREFERENCES_DEVICE_STATUS_DOT_WIDTH
        + PREFERENCES_DEVICE_TRAILING_GAP;
    let text_width = (content_width - inset * 2.0 - trailing_width).max(120.0);
    let detail_y = PREFERENCES_DEVICE_ROW_BOTTOM_INSET;
    let title_y = (PREFERENCES_DEVICE_ROW_HEIGHT
        - PREFERENCES_DEVICE_ROW_TOP_INSET
        - PREFERENCES_DEVICE_ROW_TITLE_HEIGHT)
        .max(detail_y + PREFERENCES_DEVICE_ROW_DETAIL_HEIGHT + PREFERENCES_DEVICE_ROW_LABEL_GAP);

    let title = make_history_primary_label(mtm, &row.device_name, text_width);
    title.setFrame(NSRect::new(
        NSPoint::new(inset, title_y),
        NSSize::new(text_width, PREFERENCES_DEVICE_ROW_TITLE_HEIGHT),
    ));
    title.setFont(Some(
        &NSFont::boldSystemFontOfSize(NSFont::systemFontSize()),
    ));
    if !row.device_name.trim().is_empty() {
        title.setToolTip(Some(&NSString::from_str(&row.device_name)));
    }

    let status_size = PREFERENCES_DEVICE_STATUS_DOT_WIDTH;
    let trailing_anchor = content_width - inset - PREFERENCES_DEVICE_TRAILING_SAFE_INSET;
    let status_origin_x = trailing_anchor - status_size;

    let status_frame = NSRect::new(
        NSPoint::new(
            status_origin_x,
            ((PREFERENCES_DEVICE_ROW_HEIGHT - status_size) / 2.0)
                .floor()
                .max(0.0),
        ),
        NSSize::new(status_size, status_size),
    );
    let status_dot = make_preferences_status_indicator(mtm, row, status_frame);

    let detail = NSTextField::labelWithString(&NSString::from_str(&row.secondary_text), mtm);
    detail.setFrame(NSRect::new(
        NSPoint::new(inset, detail_y),
        NSSize::new(text_width, PREFERENCES_DEVICE_ROW_DETAIL_HEIGHT),
    ));
    detail.setFont(Some(
        &NSFont::labelFontOfSize(NSFont::smallSystemFontSize()),
    ));
    detail.setTextColor(Some(&NSColor::secondaryLabelColor()));
    detail.setUsesSingleLineMode(true);
    detail.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    detail.setMaximumNumberOfLines(1);
    if let Some(cell) = detail.cell() {
        cell.setWraps(false);
        cell.setScrollable(false);
        cell.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        cell.setTruncatesLastVisibleLine(true);
    }
    if !row.secondary_text.trim().is_empty() {
        detail.setToolTip(Some(&NSString::from_str(&row.secondary_text)));
    }

    row_view.addSubview(&title);
    row_view.addSubview(&detail);
    row_view.addSubview(&status_dot);
    unsafe {
        row_view.setTextField(Some(&title));
    }
    row_view
}

fn make_preferences_status_indicator(
    mtm: MainThreadMarker,
    row: &SettingsDeviceEntry,
    frame: NSRect,
) -> Retained<NSView> {
    let (tint, status_label) = preferences_device_status_style(row);
    let indicator = NSView::initWithFrame(NSView::alloc(mtm), frame);
    indicator.setWantsLayer(true);
    let layer = CALayer::layer();
    let tint_ref: &NSColor = &tint;
    let cg_color = tint_ref.CGColor();
    layer.setBackgroundColor(Some(&cg_color));
    layer.setCornerRadius(frame.size.width / 2.0);
    layer.setMasksToBounds(true);
    indicator.setLayer(Some(&layer));
    let tooltip = if row.status_tooltip.trim().is_empty() {
        status_label.to_string()
    } else {
        format!("{status_label}\n{}", row.status_tooltip)
    };
    indicator.setToolTip(Some(&NSString::from_str(&tooltip)));
    indicator
}

fn preferences_device_status_style(row: &SettingsDeviceEntry) -> (Retained<NSColor>, &'static str) {
    match row.status_kind {
        DeviceStatusKind::ConnectedTrusted => (NSColor::systemGreenColor(), "已信任已连接"),
        DeviceStatusKind::TrustedStandby => (NSColor::systemYellowColor(), "已信任未连接"),
        DeviceStatusKind::Offline => (NSColor::systemRedColor(), "离线"),
        DeviceStatusKind::Untrusted => (NSColor::systemGrayColor(), "未信任"),
    }
}

pub(super) fn make_input_field(
    mtm: MainThreadMarker,
    frame: NSRect,
    placeholder: &str,
) -> Retained<NSTextField> {
    let field = NSTextField::initWithFrame(NSTextField::alloc(mtm), frame);
    field.setPlaceholderString(Some(&NSString::from_str(placeholder)));
    field
}

pub(super) fn make_checkbox(
    mtm: MainThreadMarker,
    title: &str,
    frame: NSRect,
) -> Retained<NSButton> {
    let button = unsafe {
        NSButton::checkboxWithTitle_target_action(&NSString::from_str(title), None, None, mtm)
    };
    button.setFrame(frame);
    button
}

pub(super) fn make_separator(mtm: MainThreadMarker, frame: NSRect) -> Retained<NSTextField> {
    let line = NSTextField::initWithFrame(NSTextField::alloc(mtm), frame);
    line.setStringValue(&NSString::from_str(""));
    line.setEditable(false);
    line.setSelectable(false);
    line.setBordered(false);
    line.setBezeled(false);
    line.setDrawsBackground(true);
    line.setBackgroundColor(Some(&NSColor::separatorColor()));
    line
}

pub(super) fn make_background_box(mtm: MainThreadMarker, frame: NSRect) -> Retained<NSTextField> {
    let box_view = NSTextField::initWithFrame(NSTextField::alloc(mtm), frame);
    box_view.setStringValue(&NSString::from_str(""));
    box_view.setEditable(false);
    box_view.setSelectable(false);
    box_view.setBordered(false);
    box_view.setBezeled(false);
    box_view.setDrawsBackground(true);
    box_view.setBackgroundColor(Some(&NSColor::controlBackgroundColor()));
    box_view
}

pub(super) fn truncate_menu_title(summary: &str) -> String {
    const MAX_CHARS: usize = 56;
    let text = summary.trim();
    if text.chars().count() <= MAX_CHARS {
        text.to_string()
    } else {
        let mut clipped = text.chars().take(MAX_CHARS - 1).collect::<String>();
        clipped.push('…');
        clipped
    }
}
