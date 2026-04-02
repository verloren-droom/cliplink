use std::{env, fs, process::Command};

/// Shared host-platform helpers for deriving a human-friendly local device name.
pub(super) fn device_name_hint() -> Option<String> {
    ["CLIPLINK_DEVICE_NAME", "HOSTNAME", "HOST", "COMPUTERNAME"]
        .into_iter()
        .filter_map(|key| env::var(key).ok())
        .find_map(normalize_device_name_hint)
        .or_else(read_macos_computer_name)
        .or_else(read_command_hostname)
        .or_else(|| read_hostname_file("/etc/hostname"))
        .or_else(|| read_hostname_file("/proc/sys/kernel/hostname"))
}

fn read_hostname_file(path: &str) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .and_then(normalize_device_name_hint)
}

#[cfg(target_os = "macos")]
fn read_macos_computer_name() -> Option<String> {
    Command::new("scutil")
        .args(["--get", "ComputerName"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(normalize_device_name_hint)
}

#[cfg(not(target_os = "macos"))]
fn read_macos_computer_name() -> Option<String> {
    None
}

fn read_command_hostname() -> Option<String> {
    Command::new("hostname")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(normalize_device_name_hint)
}

fn normalize_device_name_hint(value: String) -> Option<String> {
    let trimmed = value.trim().trim_matches('\0').to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}
