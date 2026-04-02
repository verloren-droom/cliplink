use global_hotkey::hotkey::{Code, Modifiers};

pub(super) fn format_hotkey_for_display(raw: &str) -> String {
    super::format_with(raw, " ", modifier_labels, key_label)
}

fn modifier_labels(modifiers: Modifiers) -> Vec<String> {
    let mut parts = Vec::new();
    super::push_modifier(&mut parts, modifiers, Modifiers::SUPER, "⌘");
    super::push_modifier(&mut parts, modifiers, Modifiers::CONTROL, "⌃");
    super::push_modifier(&mut parts, modifiers, Modifiers::ALT, "⌥");
    super::push_modifier(&mut parts, modifiers, Modifiers::SHIFT, "⇧");
    parts
}

fn key_label(code: Code) -> String {
    let raw = code.to_string();
    if let Some(label) = super::format_basic_key(code, raw.as_str()) {
        return label;
    }

    match code {
        Code::Space => "空格".to_string(),
        Code::Enter => "↩".to_string(),
        Code::Tab => "⇥".to_string(),
        Code::Escape => "⎋".to_string(),
        Code::Backspace => "⌫".to_string(),
        Code::Delete => "⌦".to_string(),
        Code::ArrowUp => "↑".to_string(),
        Code::ArrowDown => "↓".to_string(),
        Code::ArrowLeft => "←".to_string(),
        Code::ArrowRight => "→".to_string(),
        Code::Home => "↖".to_string(),
        Code::End => "↘".to_string(),
        Code::PageUp => "⇞".to_string(),
        Code::PageDown => "⇟".to_string(),
        _ => raw,
    }
}
