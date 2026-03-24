use std::str::FromStr;

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use windows_sys::Win32::UI::{
    Controls::{HOTKEYF_ALT, HOTKEYF_CONTROL, HOTKEYF_SHIFT},
    Input::KeyboardAndMouse::{
        MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, VK_BACK, VK_DELETE, VK_DOWN,
        VK_END, VK_ESCAPE, VK_F1, VK_F2, VK_F3, VK_F4, VK_F5, VK_F6, VK_F7, VK_F8, VK_F9, VK_F10,
        VK_F11, VK_F12, VK_F13, VK_F14, VK_F15, VK_F16, VK_F17, VK_F18, VK_F19, VK_F20, VK_F21,
        VK_F22, VK_F23, VK_F24, VK_HOME, VK_LEFT, VK_NEXT, VK_OEM_1, VK_OEM_2, VK_OEM_3, VK_OEM_4,
        VK_OEM_5, VK_OEM_6, VK_OEM_7, VK_OEM_COMMA, VK_OEM_MINUS, VK_OEM_PERIOD, VK_OEM_PLUS,
        VK_PRIOR, VK_RETURN, VK_RIGHT, VK_SPACE, VK_TAB, VK_UP,
    },
};

#[derive(Debug, Clone, Copy)]
pub(super) struct RegisteredHotKey {
    pub modifiers: u32,
    pub vkey: u32,
}

pub(super) fn parse_registered_hotkey(raw: &str) -> Result<RegisteredHotKey, String> {
    let hotkey = HotKey::from_str(raw).map_err(|error| error.to_string())?;
    let mut modifiers = MOD_NOREPEAT;
    if hotkey.mods.contains(Modifiers::CONTROL) {
        modifiers |= MOD_CONTROL;
    }
    if hotkey.mods.contains(Modifiers::SHIFT) {
        modifiers |= MOD_SHIFT;
    }
    if hotkey.mods.contains(Modifiers::ALT) {
        modifiers |= MOD_ALT;
    }
    if hotkey.mods.contains(Modifiers::SUPER) {
        modifiers |= MOD_WIN;
    }

    let Some(vkey) = code_to_vkey(hotkey.key) else {
        return Err("当前快捷键暂不支持 Windows 全局注册。".to_string());
    };

    Ok(RegisteredHotKey { modifiers, vkey })
}

pub(super) fn format_hotkey_for_display(raw: &str) -> String {
    match HotKey::from_str(raw) {
        Ok(hotkey) => {
            let mut parts = Vec::new();
            if hotkey.mods.contains(Modifiers::CONTROL) {
                parts.push("Ctrl".to_string());
            }
            if hotkey.mods.contains(Modifiers::SHIFT) {
                parts.push("Shift".to_string());
            }
            if hotkey.mods.contains(Modifiers::ALT) {
                parts.push("Alt".to_string());
            }
            if hotkey.mods.contains(Modifiers::SUPER) {
                parts.push("Win".to_string());
            }
            parts.push(format_hotkey_key_for_display(hotkey.key));
            parts.join(" + ")
        }
        Err(_) => raw.to_string(),
    }
}

pub(super) fn hotkey_control_value(raw: &str) -> Result<u16, String> {
    let hotkey = HotKey::from_str(raw).map_err(|error| error.to_string())?;
    if hotkey.mods.contains(Modifiers::SUPER) {
        return Err("Windows 快捷键设置暂不支持使用 Win 键。".to_string());
    }

    let Some(vkey) = code_to_vkey(hotkey.key) else {
        return Err("当前快捷键暂不支持 Windows 原生热键控件。".to_string());
    };

    let mut flags = 0_u16;
    if hotkey.mods.contains(Modifiers::SHIFT) {
        flags |= HOTKEYF_SHIFT as u16;
    }
    if hotkey.mods.contains(Modifiers::CONTROL) {
        flags |= HOTKEYF_CONTROL as u16;
    }
    if hotkey.mods.contains(Modifiers::ALT) {
        flags |= HOTKEYF_ALT as u16;
    }

    Ok((vkey as u16) | (flags << 8))
}

pub(super) fn hotkey_from_control_value(value: u32) -> Result<String, String> {
    let vkey = value & 0xFF;
    if vkey == 0 {
        return Err("显示历史列表快捷键不能为空。".to_string());
    }

    let Some(code) = vkey_to_code(vkey) else {
        return Err("当前快捷键暂不支持录制。".to_string());
    };

    let flags = (value >> 8) as u16;
    let mut modifiers = Modifiers::empty();
    if flags & HOTKEYF_CONTROL as u16 != 0 {
        modifiers |= Modifiers::CONTROL;
    }
    if flags & HOTKEYF_SHIFT as u16 != 0 {
        modifiers |= Modifiers::SHIFT;
    }
    if flags & HOTKEYF_ALT as u16 != 0 {
        modifiers |= Modifiers::ALT;
    }
    if modifiers.is_empty() {
        return Err("快捷键至少需要一个修饰键。".to_string());
    }

    Ok(HotKey::new(Some(modifiers), code).to_string())
}

fn format_hotkey_key_for_display(code: Code) -> String {
    let raw = code.to_string();

    if let Some(letter) = raw.strip_prefix("Key") {
        return letter.to_string();
    }
    if let Some(digit) = raw.strip_prefix("Digit") {
        return digit.to_string();
    }
    if let Some(numpad) = raw.strip_prefix("Numpad") {
        return format!("数字键盘 {numpad}");
    }

    match code {
        Code::Space => "空格".to_string(),
        Code::Enter => "Enter".to_string(),
        Code::Tab => "Tab".to_string(),
        Code::Escape => "Esc".to_string(),
        Code::Backspace => "Backspace".to_string(),
        Code::Delete => "Delete".to_string(),
        Code::ArrowUp => "↑".to_string(),
        Code::ArrowDown => "↓".to_string(),
        Code::ArrowLeft => "←".to_string(),
        Code::ArrowRight => "→".to_string(),
        Code::Home => "Home".to_string(),
        Code::End => "End".to_string(),
        Code::PageUp => "PageUp".to_string(),
        Code::PageDown => "PageDown".to_string(),
        Code::Comma => ",".to_string(),
        Code::Period => ".".to_string(),
        Code::Slash => "/".to_string(),
        Code::Semicolon => ";".to_string(),
        Code::Quote => "'".to_string(),
        Code::BracketLeft => "[".to_string(),
        Code::BracketRight => "]".to_string(),
        Code::Backslash => "\\".to_string(),
        Code::Backquote => "`".to_string(),
        Code::Minus => "-".to_string(),
        Code::Equal => "=".to_string(),
        _ => raw,
    }
}

fn code_to_vkey(code: Code) -> Option<u32> {
    let raw = code.to_string();
    if let Some(letter) = raw.strip_prefix("Key") {
        return letter.chars().next().map(|value| value as u32);
    }
    if let Some(digit) = raw.strip_prefix("Digit") {
        return digit.chars().next().map(|value| value as u32);
    }
    if let Some(function) = raw.strip_prefix('F') {
        let number = function.parse::<u32>().ok()?;
        return Some(VK_F1 as u32 + number.saturating_sub(1));
    }

    Some(match code {
        Code::Space => VK_SPACE as u32,
        Code::Enter => VK_RETURN as u32,
        Code::Tab => VK_TAB as u32,
        Code::Escape => VK_ESCAPE as u32,
        Code::Backspace => VK_BACK as u32,
        Code::Delete => VK_DELETE as u32,
        Code::ArrowUp => VK_UP as u32,
        Code::ArrowDown => VK_DOWN as u32,
        Code::ArrowLeft => VK_LEFT as u32,
        Code::ArrowRight => VK_RIGHT as u32,
        Code::Home => VK_HOME as u32,
        Code::End => VK_END as u32,
        Code::PageUp => VK_PRIOR as u32,
        Code::PageDown => VK_NEXT as u32,
        Code::Comma => VK_OEM_COMMA as u32,
        Code::Period => VK_OEM_PERIOD as u32,
        Code::Slash => VK_OEM_2 as u32,
        Code::Semicolon => VK_OEM_1 as u32,
        Code::Quote => VK_OEM_7 as u32,
        Code::BracketLeft => VK_OEM_4 as u32,
        Code::BracketRight => VK_OEM_6 as u32,
        Code::Backslash => VK_OEM_5 as u32,
        Code::Backquote => VK_OEM_3 as u32,
        Code::Minus => VK_OEM_MINUS as u32,
        Code::Equal => VK_OEM_PLUS as u32,
        _ => return None,
    })
}

fn vkey_to_code(vkey: u32) -> Option<Code> {
    if (b'A' as u32..=b'Z' as u32).contains(&vkey) {
        return Code::from_str(&format!("Key{}", char::from_u32(vkey)?)).ok();
    }
    if (b'0' as u32..=b'9' as u32).contains(&vkey) {
        return Code::from_str(&format!("Digit{}", char::from_u32(vkey)?)).ok();
    }

    Some(match vkey {
        value if value == VK_SPACE as u32 => Code::Space,
        value if value == VK_RETURN as u32 => Code::Enter,
        value if value == VK_TAB as u32 => Code::Tab,
        value if value == VK_ESCAPE as u32 => Code::Escape,
        value if value == VK_BACK as u32 => Code::Backspace,
        value if value == VK_DELETE as u32 => Code::Delete,
        value if value == VK_UP as u32 => Code::ArrowUp,
        value if value == VK_DOWN as u32 => Code::ArrowDown,
        value if value == VK_LEFT as u32 => Code::ArrowLeft,
        value if value == VK_RIGHT as u32 => Code::ArrowRight,
        value if value == VK_HOME as u32 => Code::Home,
        value if value == VK_END as u32 => Code::End,
        value if value == VK_PRIOR as u32 => Code::PageUp,
        value if value == VK_NEXT as u32 => Code::PageDown,
        value if value == VK_OEM_COMMA as u32 => Code::Comma,
        value if value == VK_OEM_PERIOD as u32 => Code::Period,
        value if value == VK_OEM_2 as u32 => Code::Slash,
        value if value == VK_OEM_1 as u32 => Code::Semicolon,
        value if value == VK_OEM_7 as u32 => Code::Quote,
        value if value == VK_OEM_4 as u32 => Code::BracketLeft,
        value if value == VK_OEM_6 as u32 => Code::BracketRight,
        value if value == VK_OEM_5 as u32 => Code::Backslash,
        value if value == VK_OEM_3 as u32 => Code::Backquote,
        value if value == VK_OEM_MINUS as u32 => Code::Minus,
        value if value == VK_OEM_PLUS as u32 => Code::Equal,
        value if value == VK_F1 as u32 => Code::F1,
        value if value == VK_F2 as u32 => Code::F2,
        value if value == VK_F3 as u32 => Code::F3,
        value if value == VK_F4 as u32 => Code::F4,
        value if value == VK_F5 as u32 => Code::F5,
        value if value == VK_F6 as u32 => Code::F6,
        value if value == VK_F7 as u32 => Code::F7,
        value if value == VK_F8 as u32 => Code::F8,
        value if value == VK_F9 as u32 => Code::F9,
        value if value == VK_F10 as u32 => Code::F10,
        value if value == VK_F11 as u32 => Code::F11,
        value if value == VK_F12 as u32 => Code::F12,
        value if value == VK_F13 as u32 => Code::F13,
        value if value == VK_F14 as u32 => Code::F14,
        value if value == VK_F15 as u32 => Code::F15,
        value if value == VK_F16 as u32 => Code::F16,
        value if value == VK_F17 as u32 => Code::F17,
        value if value == VK_F18 as u32 => Code::F18,
        value if value == VK_F19 as u32 => Code::F19,
        value if value == VK_F20 as u32 => Code::F20,
        value if value == VK_F21 as u32 => Code::F21,
        value if value == VK_F22 as u32 => Code::F22,
        value if value == VK_F23 as u32 => Code::F23,
        value if value == VK_F24 as u32 => Code::F24,
        _ => return None,
    })
}
