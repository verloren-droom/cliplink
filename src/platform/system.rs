use arboard::Clipboard;
use parking_lot::Mutex;

use crate::core::error::AppResult;
use crate::core::{
    clipboard::{ClipboardBackend, build_item_from_text},
    error::AppError,
    model::ClipboardItem,
};

pub(super) fn create_clipboard_backend() -> AppResult<Box<dyn ClipboardBackend>> {
    Ok(Box::new(SystemClipboard::new()?))
}

struct SystemClipboard {
    clipboard: Mutex<Clipboard>,
    last_signature: Mutex<Option<String>>,
}

impl SystemClipboard {
    fn new() -> AppResult<Self> {
        let clipboard = Clipboard::new().map_err(|error| AppError::Clipboard(error.to_string()))?;
        Ok(Self {
            clipboard: Mutex::new(clipboard),
            last_signature: Mutex::new(None),
        })
    }
}

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
