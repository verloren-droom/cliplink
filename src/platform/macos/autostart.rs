use std::{
    env, fs,
    path::{Path, PathBuf},
};

use crate::constants::app::APP_BUNDLE_ID;
use crate::core::error::{AppError, AppResult};

pub(super) fn launch_at_login_enabled() -> AppResult<bool> {
    Ok(launch_agent_path()?.exists())
}

pub(super) fn set_launch_at_login(enabled: bool) -> AppResult<()> {
    let plist_path = launch_agent_path()?;

    if !enabled {
        if plist_path.exists() {
            fs::remove_file(plist_path)?;
        }
        return Ok(());
    }

    let executable = env::current_exe()?;
    let parent = plist_path.parent().ok_or_else(|| {
        AppError::InvalidConfig("LaunchAgent directory is not available.".to_string())
    })?;
    fs::create_dir_all(parent)?;

    let contents = launch_agent_plist(&executable);
    let temp_path = plist_path.with_extension("plist.tmp");
    fs::write(&temp_path, contents.as_bytes())?;
    fs::rename(temp_path, plist_path)?;
    Ok(())
}

fn launch_agent_path() -> AppResult<PathBuf> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| AppError::InvalidConfig("HOME is not set.".to_string()))?;
    Ok(home
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{}.plist", launch_agent_label())))
}

fn launch_agent_label() -> String {
    format!("{APP_BUNDLE_ID}.ui")
}

fn launch_agent_plist(executable: &Path) -> String {
    let label = xml_escape(&launch_agent_label());
    let executable = xml_escape(executable.to_string_lossy().as_ref());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>LimitLoadToSessionType</key>
    <string>Aqua</string>
    <key>ProcessType</key>
    <string>Interactive</string>
    <key>ProgramArguments</key>
    <array>
        <string>{executable}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <false/>
</dict>
</plist>
"#
    )
}

fn xml_escape(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&apos;"),
            _ => output.push(ch),
        }
    }
    output
}
