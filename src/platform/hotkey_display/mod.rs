use global_hotkey::hotkey::{Code, HotKey, Modifiers};

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
use self::macos as platform_impl;

#[cfg(target_os = "windows")]
use self::windows as platform_impl;

pub(crate) fn format_hotkey_for_display(raw: &str) -> String {
    platform_impl::format_hotkey_for_display(raw)
}

fn format_with(
    raw: &str,
    separator: &str,
    modifier_formatter: fn(Modifiers) -> Vec<String>,
    key_formatter: fn(Code) -> String,
) -> String {
    match raw.parse::<HotKey>() {
        Ok(hotkey) => {
            let mut parts = modifier_formatter(hotkey.mods);
            parts.push(key_formatter(hotkey.key));
            parts.join(separator)
        }
        Err(_) => raw.to_string(),
    }
}

fn push_modifier(parts: &mut Vec<String>, modifiers: Modifiers, flag: Modifiers, label: &str) {
    if modifiers.contains(flag) {
        parts.push(label.to_string());
    }
}

fn format_basic_key(code: Code, raw: &str) -> Option<String> {
    if let Some(letter) = raw.strip_prefix("Key") {
        return Some(letter.to_string());
    }
    if let Some(digit) = raw.strip_prefix("Digit") {
        return Some(digit.to_string());
    }
    if let Some(numpad) = raw.strip_prefix("Numpad") {
        return Some(format!("数字键盘 {numpad}"));
    }

    match code {
        Code::Comma => Some(",".to_string()),
        Code::Period => Some(".".to_string()),
        Code::Slash => Some("/".to_string()),
        Code::Semicolon => Some(";".to_string()),
        Code::Quote => Some("'".to_string()),
        Code::BracketLeft => Some("[".to_string()),
        Code::BracketRight => Some("]".to_string()),
        Code::Backslash => Some("\\".to_string()),
        Code::Backquote => Some("`".to_string()),
        Code::Minus => Some("-".to_string()),
        Code::Equal => Some("=".to_string()),
        _ => None,
    }
}
