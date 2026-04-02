#[cfg(target_os = "android")]
mod android;

mod current;

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod hotkey_display;

#[cfg(all(
    not(target_os = "macos"),
    not(target_os = "android"),
    not(target_os = "windows")
))]
mod keyfile;

#[cfg(not(target_os = "android"))]
mod host_identity;

#[cfg(not(target_os = "android"))]
mod host_paths;

#[cfg(all(
    not(target_os = "macos"),
    not(target_os = "android"),
    not(target_os = "windows")
))]
mod system;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "windows")]
mod windows;

pub(crate) type PlatformResult = Result<(), Box<dyn std::error::Error>>;

pub(crate) use self::current::{
    controller_runtime_policy, create_clipboard_backend, create_local_data_cipher,
    device_name_hint, discover_app_paths, history_store_profile, run,
};
