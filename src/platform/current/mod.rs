#[cfg(target_os = "android")]
mod android;

#[cfg(all(
    not(target_os = "macos"),
    not(target_os = "android"),
    not(target_os = "windows")
))]
mod fallback;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "android")]
pub(crate) use self::android::{
    controller_runtime_policy, create_clipboard_backend, create_local_data_cipher,
    device_name_hint, discover_app_paths, history_store_profile, run,
};

#[cfg(all(
    not(target_os = "macos"),
    not(target_os = "android"),
    not(target_os = "windows")
))]
pub(crate) use self::fallback::{
    controller_runtime_policy, create_clipboard_backend, create_local_data_cipher,
    device_name_hint, discover_app_paths, history_store_profile, run,
};

#[cfg(target_os = "macos")]
pub(crate) use self::macos::{
    controller_runtime_policy, create_clipboard_backend, create_local_data_cipher,
    device_name_hint, discover_app_paths, history_store_profile, run,
};

#[cfg(target_os = "windows")]
pub(crate) use self::windows::{
    controller_runtime_policy, create_clipboard_backend, create_local_data_cipher,
    device_name_hint, discover_app_paths, history_store_profile, run,
};
