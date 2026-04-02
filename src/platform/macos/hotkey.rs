use std::str::FromStr;

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use objc2_app_kit::{
    NSDeleteFunctionKey, NSDownArrowFunctionKey, NSEndFunctionKey, NSEvent, NSEventModifierFlags,
    NSF1FunctionKey, NSF2FunctionKey, NSF3FunctionKey, NSF4FunctionKey, NSF5FunctionKey,
    NSF6FunctionKey, NSF7FunctionKey, NSF8FunctionKey, NSF9FunctionKey, NSF10FunctionKey,
    NSF11FunctionKey, NSF12FunctionKey, NSF13FunctionKey, NSF14FunctionKey, NSF15FunctionKey,
    NSF16FunctionKey, NSF17FunctionKey, NSF18FunctionKey, NSF19FunctionKey, NSF20FunctionKey,
    NSF21FunctionKey, NSF22FunctionKey, NSF23FunctionKey, NSF24FunctionKey, NSHomeFunctionKey,
    NSLeftArrowFunctionKey, NSPageDownFunctionKey, NSPageUpFunctionKey, NSRightArrowFunctionKey,
    NSUpArrowFunctionKey,
};

use crate::platform::hotkey_display::format_hotkey_for_display as format_hotkey_for_display_impl;

pub(super) fn format_hotkey_for_display(raw: &str) -> String {
    format_hotkey_for_display_impl(raw)
}

pub(super) fn hotkey_from_event(event: &NSEvent) -> Result<Option<HotKey>, &'static str> {
    let Some(chars) = event.charactersIgnoringModifiers() else {
        return Err("当前按键暂不支持录制。");
    };
    let Some(ch) = chars.to_string().chars().next() else {
        return Err("当前按键暂不支持录制。");
    };

    let flags = event.modifierFlags() & NSEventModifierFlags::DeviceIndependentFlagsMask;
    let mut mods = Modifiers::empty();
    if flags.contains(NSEventModifierFlags::Shift) {
        mods |= Modifiers::SHIFT;
    }
    if flags.contains(NSEventModifierFlags::Control) {
        mods |= Modifiers::CONTROL;
    }
    if flags.contains(NSEventModifierFlags::Option) {
        mods |= Modifiers::ALT;
    }
    if flags.contains(NSEventModifierFlags::Command) {
        mods |= Modifiers::SUPER;
    }

    let Some(code) = code_from_key_char(ch) else {
        return Err("当前按键暂不支持录制。");
    };

    if code == Code::Escape && mods.is_empty() {
        return Ok(None);
    }

    if mods.is_empty() {
        return Err("快捷键至少需要一个修饰键。");
    }

    Ok(Some(HotKey::new(Some(mods), code)))
}
fn code_from_key_char(ch: char) -> Option<Code> {
    if ch.is_ascii_alphabetic() {
        return Code::from_str(&format!("Key{}", ch.to_ascii_uppercase())).ok();
    }

    if ch.is_ascii_digit() {
        return Code::from_str(&format!("Digit{ch}")).ok();
    }

    match ch {
        ' ' => Some(Code::Space),
        ',' => Some(Code::Comma),
        '.' => Some(Code::Period),
        '/' => Some(Code::Slash),
        ';' => Some(Code::Semicolon),
        '\'' => Some(Code::Quote),
        '[' => Some(Code::BracketLeft),
        ']' => Some(Code::BracketRight),
        '\\' => Some(Code::Backslash),
        '`' => Some(Code::Backquote),
        '-' => Some(Code::Minus),
        '=' => Some(Code::Equal),
        '\r' | '\n' => Some(Code::Enter),
        '\t' => Some(Code::Tab),
        '\u{8}' | '\u{7f}' => Some(Code::Backspace),
        '\u{1b}' => Some(Code::Escape),
        value if value as u32 == NSUpArrowFunctionKey => Some(Code::ArrowUp),
        value if value as u32 == NSDownArrowFunctionKey => Some(Code::ArrowDown),
        value if value as u32 == NSLeftArrowFunctionKey => Some(Code::ArrowLeft),
        value if value as u32 == NSRightArrowFunctionKey => Some(Code::ArrowRight),
        value if value as u32 == NSDeleteFunctionKey => Some(Code::Delete),
        value if value as u32 == NSHomeFunctionKey => Some(Code::Home),
        value if value as u32 == NSEndFunctionKey => Some(Code::End),
        value if value as u32 == NSPageUpFunctionKey => Some(Code::PageUp),
        value if value as u32 == NSPageDownFunctionKey => Some(Code::PageDown),
        value if value as u32 == NSF1FunctionKey => Some(Code::F1),
        value if value as u32 == NSF2FunctionKey => Some(Code::F2),
        value if value as u32 == NSF3FunctionKey => Some(Code::F3),
        value if value as u32 == NSF4FunctionKey => Some(Code::F4),
        value if value as u32 == NSF5FunctionKey => Some(Code::F5),
        value if value as u32 == NSF6FunctionKey => Some(Code::F6),
        value if value as u32 == NSF7FunctionKey => Some(Code::F7),
        value if value as u32 == NSF8FunctionKey => Some(Code::F8),
        value if value as u32 == NSF9FunctionKey => Some(Code::F9),
        value if value as u32 == NSF10FunctionKey => Some(Code::F10),
        value if value as u32 == NSF11FunctionKey => Some(Code::F11),
        value if value as u32 == NSF12FunctionKey => Some(Code::F12),
        value if value as u32 == NSF13FunctionKey => Some(Code::F13),
        value if value as u32 == NSF14FunctionKey => Some(Code::F14),
        value if value as u32 == NSF15FunctionKey => Some(Code::F15),
        value if value as u32 == NSF16FunctionKey => Some(Code::F16),
        value if value as u32 == NSF17FunctionKey => Some(Code::F17),
        value if value as u32 == NSF18FunctionKey => Some(Code::F18),
        value if value as u32 == NSF19FunctionKey => Some(Code::F19),
        value if value as u32 == NSF20FunctionKey => Some(Code::F20),
        value if value as u32 == NSF21FunctionKey => Some(Code::F21),
        value if value as u32 == NSF22FunctionKey => Some(Code::F22),
        value if value as u32 == NSF23FunctionKey => Some(Code::F23),
        value if value as u32 == NSF24FunctionKey => Some(Code::F24),
        _ => None,
    }
}
