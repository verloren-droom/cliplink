use crate::controller::AppController;
use crate::core::{
    at_rest::LocalDataCipher, clipboard::ClipboardBackend, error::AppResult, paths::AppPaths,
};

#[cfg(target_os = "android")]
use crate::core::error::AppError;

#[cfg(target_os = "android")]
mod android;

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
mod keyfile;

#[cfg(all(not(target_os = "android"), not(target_os = "windows")))]
mod system;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "windows")]
mod windows;

pub type PlatformResult = Result<(), Box<dyn std::error::Error>>;

#[cfg(all(not(target_os = "android"), not(target_os = "windows")))]
pub fn device_name_hint() -> Option<String> {
    system::device_name_hint()
}

#[cfg(target_os = "android")]
pub fn device_name_hint() -> Option<String> {
    None
}

#[cfg(target_os = "windows")]
pub fn device_name_hint() -> Option<String> {
    windows::device_name_hint()
}

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "macos"),
    not(target_os = "windows")
))]
pub fn create_clipboard_backend() -> AppResult<Box<dyn ClipboardBackend>> {
    system::create_clipboard_backend()
}

#[cfg(target_os = "macos")]
pub fn create_clipboard_backend() -> AppResult<Box<dyn ClipboardBackend>> {
    macos::create_clipboard_backend()
}

#[cfg(target_os = "android")]
pub fn create_clipboard_backend() -> AppResult<Box<dyn ClipboardBackend>> {
    Err(AppError::InvalidConfig(
        "Android uses the native bridge bootstrap instead of the desktop clipboard backend."
            .to_string(),
    ))
}

#[cfg(target_os = "windows")]
pub fn create_clipboard_backend() -> AppResult<Box<dyn ClipboardBackend>> {
    windows::create_clipboard_backend()
}

#[cfg(all(not(target_os = "android"), not(target_os = "windows")))]
pub fn discover_app_paths() -> AppResult<AppPaths> {
    system::discover_app_paths()
}

#[cfg(target_os = "android")]
pub fn discover_app_paths() -> AppResult<AppPaths> {
    Err(AppError::InvalidConfig(
        "Android paths must be provided by the native bridge.".to_string(),
    ))
}

#[cfg(target_os = "windows")]
pub fn discover_app_paths() -> AppResult<AppPaths> {
    windows::discover_app_paths()
}

#[cfg(target_os = "macos")]
pub fn create_local_data_cipher(paths: &AppPaths) -> AppResult<LocalDataCipher> {
    macos::create_local_data_cipher(paths)
}

#[cfg(target_os = "android")]
pub fn create_local_data_cipher(paths: &AppPaths) -> AppResult<LocalDataCipher> {
    android::create_local_data_cipher(paths)
}

#[cfg(target_os = "windows")]
pub fn create_local_data_cipher(paths: &AppPaths) -> AppResult<LocalDataCipher> {
    windows::create_local_data_cipher(paths)
}

#[cfg(all(
    not(target_os = "macos"),
    not(target_os = "android"),
    not(target_os = "windows")
))]
pub fn create_local_data_cipher(paths: &AppPaths) -> AppResult<LocalDataCipher> {
    keyfile::create_local_data_cipher(paths)
}

pub fn run(controller: AppController) -> PlatformResult {
    #[cfg(target_os = "macos")]
    {
        macos::run(controller)
    }

    #[cfg(target_os = "android")]
    {
        android::run(controller)
    }

    #[cfg(target_os = "windows")]
    {
        windows::run(controller)
    }

    #[cfg(not(any(target_os = "macos", target_os = "android", target_os = "windows")))]
    {
        Err("The native UI is currently implemented only on macOS.".into())
    }
}
