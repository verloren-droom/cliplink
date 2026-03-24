use std::{env, path::PathBuf};

use agnostic_mdns::hostname;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use arboard::Clipboard;
use directories::ProjectDirs;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use parking_lot::Mutex;

use crate::constants::app::{
    ORGANIZATION_NAME, ORGANIZATION_QUALIFIER, PRODUCT_DIR_NAME, STORAGE_DIR_NAME,
};
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use crate::core::{
    clipboard::{ClipboardBackend, build_item_from_text},
    error::AppError,
    model::ClipboardItem,
};
use crate::core::{error::AppResult, paths::AppPaths};

pub fn device_name_hint() -> Option<String> {
    hostname()
        .map(|value| value.to_string())
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn create_clipboard_backend() -> AppResult<Box<dyn ClipboardBackend>> {
    Ok(Box::new(SystemClipboard::new()?))
}

pub fn discover_app_paths() -> AppResult<AppPaths> {
    let root = ProjectDirs::from(ORGANIZATION_QUALIFIER, ORGANIZATION_NAME, PRODUCT_DIR_NAME)
        .map(|dirs| dirs.data_local_dir().to_path_buf())
        .unwrap_or_else(|| {
            env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(format!(".{STORAGE_DIR_NAME}-data"))
        });

    let paths = AppPaths::from_root(root);
    paths.ensure()?;
    Ok(paths)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
struct SystemClipboard {
    clipboard: Mutex<Clipboard>,
    last_signature: Mutex<Option<String>>,
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl SystemClipboard {
    fn new() -> AppResult<Self> {
        let clipboard = Clipboard::new().map_err(|error| AppError::Clipboard(error.to_string()))?;
        Ok(Self {
            clipboard: Mutex::new(clipboard),
            last_signature: Mutex::new(None),
        })
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl ClipboardBackend for SystemClipboard {
    fn poll(
        &self,
        source_device_id: &str,
        source_device_name: &str,
    ) -> AppResult<Option<ClipboardItem>> {
        let text = match self.clipboard.lock().get_text() {
            Ok(text) => text,
            Err(_) => return Ok(None),
        };
        let text = text.trim().to_string();
        if text.is_empty() {
            return Ok(None);
        }

        let item = build_item_from_text(
            &text,
            Some(source_device_id.to_string()),
            Some(source_device_name.to_string()),
            false,
        );
        if self.last_signature.lock().as_ref() == Some(&item.signature) {
            return Ok(None);
        }

        *self.last_signature.lock() = Some(item.signature.clone());
        Ok(Some(item))
    }

    fn write_item(&self, item: &ClipboardItem) -> AppResult<()> {
        self.clipboard
            .lock()
            .set_text(item.as_clipboard_text())
            .map_err(|error| AppError::Clipboard(error.to_string()))?;
        *self.last_signature.lock() = Some(item.signature.clone());
        Ok(())
    }
}
