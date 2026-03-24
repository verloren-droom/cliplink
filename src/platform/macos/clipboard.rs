use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardWriting};
use objc2_foundation::{NSArray, NSInteger, NSString, NSURL};
use parking_lot::Mutex;

use crate::{
    constants::timing::MACOS_CLIPBOARD_POLL_INTERVAL,
    core::{
        clipboard::{ClipboardBackend, build_item_from_paths, build_item_from_text},
        error::{AppError, AppResult},
        model::{ClipboardItem, ClipboardPayload},
    },
};

pub(super) fn create_clipboard_backend() -> AppResult<Box<dyn ClipboardBackend>> {
    Ok(Box::new(MacClipboard::default()))
}

#[derive(Default)]
struct MacClipboard {
    last_signature: Mutex<Option<String>>,
    last_change_count: Mutex<NSInteger>,
}

impl ClipboardBackend for MacClipboard {
    fn recommended_poll_interval(&self) -> Duration {
        MACOS_CLIPBOARD_POLL_INTERVAL
    }

    fn poll(
        &self,
        source_device_id: &str,
        source_device_name: &str,
    ) -> AppResult<Option<ClipboardItem>> {
        let pasteboard = NSPasteboard::generalPasteboard();
        let change_count = pasteboard.changeCount();
        {
            let mut last_change_count = self.last_change_count.lock();
            if *last_change_count == change_count {
                return Ok(None);
            }
            *last_change_count = change_count;
        }

        let item = self.read_item(&pasteboard, source_device_id, source_device_name);
        let Some(item) = item else {
            return Ok(None);
        };

        if self.last_signature.lock().as_ref() == Some(&item.signature) {
            return Ok(None);
        }

        *self.last_signature.lock() = Some(item.signature.clone());
        Ok(Some(item))
    }

    fn write_item(&self, item: &ClipboardItem) -> AppResult<()> {
        let pasteboard = NSPasteboard::generalPasteboard();
        let _ = pasteboard.clearContents();

        match &item.payload {
            ClipboardPayload::Text(text) => {
                if !pasteboard
                    .setString_forType(&NSString::from_str(text), string_pasteboard_type())
                {
                    return Err(AppError::Clipboard(
                        "Failed to write text to the macOS pasteboard.".to_string(),
                    ));
                }
            }
            ClipboardPayload::Files(files) => {
                let file_urls = files
                    .iter()
                    .filter_map(|file| {
                        file.local_path
                            .as_ref()
                            .or(file.source_path.as_ref())
                            .filter(|path| path.exists())
                            .and_then(|path| path.to_str())
                            .map(|path| {
                                ProtocolObject::<dyn NSPasteboardWriting>::from_retained(
                                    NSURL::fileURLWithPath(&NSString::from_str(path)),
                                )
                            })
                    })
                    .collect::<Vec<_>>();

                if file_urls.is_empty() {
                    return Err(AppError::Clipboard(
                        "File clipboard item is missing readable file paths.".to_string(),
                    ));
                }

                let file_array = NSArray::from_retained_slice(&file_urls);
                let wrote_files = pasteboard.writeObjects(&file_array);
                let wrote_text = pasteboard.setString_forType(
                    &NSString::from_str(&item.as_clipboard_text()),
                    string_pasteboard_type(),
                );

                if !wrote_files {
                    let path_strings = files
                        .iter()
                        .filter_map(|file| {
                            file.local_path
                                .as_ref()
                                .or(file.source_path.as_ref())
                                .filter(|path| path.exists())
                                .map(|path| path_to_nsstring(path.as_path()))
                        })
                        .collect::<Vec<_>>();
                    let legacy_file_array = NSArray::from_retained_slice(&path_strings);
                    let wrote_legacy_files = unsafe {
                        pasteboard.setPropertyList_forType(
                            legacy_file_array.as_ref(),
                            legacy_filenames_pasteboard_type(),
                        )
                    };
                    if !wrote_legacy_files && !wrote_text {
                        return Err(AppError::Clipboard(
                            "Failed to write file paths to the macOS pasteboard.".to_string(),
                        ));
                    }
                }
            }
        }

        *self.last_signature.lock() = Some(item.signature.clone());
        *self.last_change_count.lock() = pasteboard.changeCount();
        Ok(())
    }
}

impl MacClipboard {
    fn read_item(
        &self,
        pasteboard: &NSPasteboard,
        source_device_id: &str,
        source_device_name: &str,
    ) -> Option<ClipboardItem> {
        self.read_file_item(pasteboard, source_device_id, source_device_name)
            .or_else(|| self.read_text_item(pasteboard, source_device_id, source_device_name))
    }

    fn read_file_item(
        &self,
        pasteboard: &NSPasteboard,
        source_device_id: &str,
        source_device_name: &str,
    ) -> Option<ClipboardItem> {
        let from_items = pasteboard
            .pasteboardItems()
            .map(|items| file_paths_from_pasteboard_items(&items))
            .unwrap_or_default();
        let paths = if from_items.is_empty() {
            legacy_file_paths_from_pasteboard(pasteboard)
        } else {
            from_items
        };

        if paths.is_empty() {
            return None;
        }

        build_item_from_paths(
            &paths,
            Some(source_device_id.to_string()),
            Some(source_device_name.to_string()),
            false,
        )
    }

    fn read_text_item(
        &self,
        pasteboard: &NSPasteboard,
        source_device_id: &str,
        source_device_name: &str,
    ) -> Option<ClipboardItem> {
        let text = pasteboard.stringForType(string_pasteboard_type())?;
        let text = text.to_string();
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return None;
        }

        Some(build_item_from_text(
            trimmed,
            Some(source_device_id.to_string()),
            Some(source_device_name.to_string()),
            false,
        ))
    }
}

fn file_paths_from_pasteboard_items(items: &NSArray<NSPasteboardItem>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for item in items {
        let Some(url_string) = item.stringForType(file_url_pasteboard_type()) else {
            continue;
        };
        let Some(url) = NSURL::URLWithString(&url_string) else {
            continue;
        };
        let file_url = url.filePathURL().unwrap_or(url);
        let Some(path) = file_url.path() else {
            continue;
        };
        let path = PathBuf::from(path.to_string());
        if path.exists() {
            paths.push(path);
        }
    }
    paths
}

fn legacy_file_paths_from_pasteboard(pasteboard: &NSPasteboard) -> Vec<PathBuf> {
    let Some(property_list) = pasteboard.propertyListForType(legacy_filenames_pasteboard_type())
    else {
        return Vec::new();
    };
    let Ok(file_array) = property_list.downcast::<NSArray>() else {
        return Vec::new();
    };
    let file_array = unsafe { file_array.cast_unchecked::<NSString>() };

    file_array
        .iter()
        .map(|value| PathBuf::from(value.to_string()))
        .filter(|path| path.exists())
        .collect()
}

fn path_to_nsstring(path: &Path) -> Retained<NSString> {
    NSString::from_str(path.to_string_lossy().as_ref())
}

fn string_pasteboard_type() -> &'static objc2_app_kit::NSPasteboardType {
    unsafe { objc2_app_kit::NSPasteboardTypeString }
}

fn file_url_pasteboard_type() -> &'static objc2_app_kit::NSPasteboardType {
    unsafe { objc2_app_kit::NSPasteboardTypeFileURL }
}

#[allow(deprecated)]
fn legacy_filenames_pasteboard_type() -> &'static objc2_app_kit::NSPasteboardType {
    unsafe { objc2_app_kit::NSFilenamesPboardType }
}
