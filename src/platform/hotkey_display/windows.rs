use global_hotkey::hotkey::{Code, Modifiers};

pub(super) fn format_hotkey_for_display(raw: &str) -> String {
    super::format_with(raw, " + ", modifier_labels, key_label)
}

fn modifier_labels(modifiers: Modifiers) -> Vec<String> {
    let mut parts = Vec::new();
    super::push_modifier(&mut parts, modifiers, Modifiers::CONTROL, "Ctrl");
    super::push_modifier(&mut parts, modifiers, Modifiers::SHIFT, "Shift");
    super::push_modifier(&mut parts, modifiers, Modifiers::ALT, "Alt");
    super::push_modifier(&mut parts, modifiers, Modifiers::SUPER, "Win");
    parts
}

fn key_label(code: Code) -> String {
    let raw = code.to_string();
    if let Some(label) = super::format_basic_key(code, raw.as_str()) {
        return label;
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
        _ => raw,
    }
}
