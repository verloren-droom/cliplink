use std::{
    mem::{size_of, zeroed},
    path::PathBuf,
    ptr::{copy_nonoverlapping, null_mut},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

use parking_lot::Mutex;
use windows_sys::Win32::{
    Foundation::{HWND, POINT},
    System::{
        DataExchange::{
            CF_UNICODETEXT, CloseClipboard, EmptyClipboard, GetClipboardData,
            IsClipboardFormatAvailable, OpenClipboard, SetClipboardData,
        },
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalFree, GlobalLock, GlobalUnlock, HGLOBAL},
    },
    UI::Shell::{DROPFILES, DragQueryFileW, HDROP},
};

use crate::core::{
    clipboard::{ClipboardBackend, build_item_from_paths, build_item_from_text},
    error::{AppError, AppResult},
    model::{ClipboardItem, ClipboardPayload},
};

/// Standard Windows clipboard format for file drops (`CF_HDROP`).
const CF_HDROP_FORMAT: u32 = 15;

static WINDOWS_CLIPBOARD_STATE: OnceLock<Arc<WindowsClipboardState>> = OnceLock::new();

pub(super) fn create_clipboard_backend() -> AppResult<Box<dyn ClipboardBackend>> {
    Ok(Box::new(WindowsClipboard::new(shared_clipboard_state())))
}

pub(super) fn shared_clipboard_state() -> Arc<WindowsClipboardState> {
    WINDOWS_CLIPBOARD_STATE
        .get_or_init(|| Arc::new(WindowsClipboardState::new()))
        .clone()
}

/// Shared Windows clipboard change marker updated from the UI thread.
pub(super) struct WindowsClipboardState {
    generation: AtomicU64,
}

impl WindowsClipboardState {
    fn new() -> Self {
        Self {
            generation: AtomicU64::new(1),
        }
    }

    pub(super) fn notify_changed(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }
}

struct WindowsClipboard {
    state: Arc<WindowsClipboardState>,
    last_seen_generation: AtomicU64,
    last_signature: Mutex<Option<String>>,
}

impl WindowsClipboard {
    fn new(state: Arc<WindowsClipboardState>) -> Self {
        Self {
            state,
            last_seen_generation: AtomicU64::new(0),
            last_signature: Mutex::new(None),
        }
    }

    fn read_item(
        &self,
        source_device_id: &str,
        source_device_name: &str,
    ) -> AppResult<Option<ClipboardItem>> {
        let _guard = ClipboardGuard::open()?;

        if let Some(paths) = read_file_paths() {
            if let Some(item) = build_item_from_paths(
                &paths,
                Some(source_device_id.to_string()),
                Some(source_device_name.to_string()),
                false,
            ) {
                return Ok(Some(item));
            }
        }

        let Some(text) = read_unicode_text() else {
            return Ok(None);
        };
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }

        Ok(Some(build_item_from_text(
            trimmed,
            Some(source_device_id.to_string()),
            Some(source_device_name.to_string()),
            false,
        )))
    }
}

impl ClipboardBackend for WindowsClipboard {
    fn poll(
        &self,
        source_device_id: &str,
        source_device_name: &str,
    ) -> AppResult<Option<ClipboardItem>> {
        let generation = self.state.generation.load(Ordering::Relaxed);
        if generation <= self.last_seen_generation.load(Ordering::Relaxed) {
            return Ok(None);
        }
        self.last_seen_generation
            .store(generation, Ordering::Relaxed);

        let Some(item) = self.read_item(source_device_id, source_device_name)? else {
            return Ok(None);
        };

        if self.last_signature.lock().as_ref() == Some(&item.signature) {
            return Ok(None);
        }

        *self.last_signature.lock() = Some(item.signature.clone());
        Ok(Some(item))
    }

    fn write_item(&self, item: &ClipboardItem) -> AppResult<()> {
        let _guard = ClipboardGuard::open()?;

        unsafe {
            if EmptyClipboard() == 0 {
                return Err(last_clipboard_error(
                    "Failed to clear the Windows clipboard.",
                ));
            }
        }

        match &item.payload {
            ClipboardPayload::Text(text) => {
                write_unicode_text(text)?;
            }
            ClipboardPayload::Files(files) => {
                let paths = files
                    .iter()
                    .filter_map(|file| {
                        file.local_path
                            .as_ref()
                            .or(file.source_path.as_ref())
                            .filter(|path| path.exists())
                            .map(|path| path.to_path_buf())
                    })
                    .collect::<Vec<_>>();

                if paths.is_empty() {
                    write_unicode_text(&item.as_clipboard_text())?;
                } else {
                    write_file_drop_list(&paths)?;
                    write_unicode_text(&item.as_clipboard_text())?;
                }
            }
        }

        *self.last_signature.lock() = Some(item.signature.clone());
        Ok(())
    }
}

struct ClipboardGuard;

impl ClipboardGuard {
    fn open() -> AppResult<Self> {
        unsafe {
            if OpenClipboard(0) == 0 {
                return Err(last_clipboard_error(
                    "Failed to open the Windows clipboard.",
                ));
            }
        }
        Ok(Self)
    }
}

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

fn read_unicode_text() -> Option<String> {
    unsafe {
        if IsClipboardFormatAvailable(CF_UNICODETEXT) == 0 {
            return None;
        }

        let handle = GetClipboardData(CF_UNICODETEXT);
        if handle == 0 {
            return None;
        }

        let ptr = GlobalLock(handle as HGLOBAL) as *const u16;
        if ptr.is_null() {
            return None;
        }

        let mut len = 0;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        let text = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
        let _ = GlobalUnlock(handle as HGLOBAL);
        Some(text)
    }
}

fn read_file_paths() -> Option<Vec<PathBuf>> {
    unsafe {
        if IsClipboardFormatAvailable(CF_HDROP_FORMAT) == 0 {
            return None;
        }

        let handle = GetClipboardData(CF_HDROP_FORMAT);
        if handle == 0 {
            return None;
        }

        let drop_handle = handle as HDROP;
        let count = DragQueryFileW(drop_handle, u32::MAX, null_mut(), 0);
        if count == 0 {
            return None;
        }

        let mut paths = Vec::with_capacity(count as usize);
        for index in 0..count {
            let len = DragQueryFileW(drop_handle, index, null_mut(), 0);
            if len == 0 {
                continue;
            }

            let mut buffer = vec![0_u16; len as usize + 1];
            let copied =
                DragQueryFileW(drop_handle, index, buffer.as_mut_ptr(), buffer.len() as u32);
            if copied == 0 {
                continue;
            }

            let path = String::from_utf16_lossy(&buffer[..copied as usize]);
            if !path.trim().is_empty() {
                paths.push(PathBuf::from(path));
            }
        }

        if paths.is_empty() { None } else { Some(paths) }
    }
}

fn write_unicode_text(text: &str) -> AppResult<()> {
    let mut wide = text.encode_utf16().collect::<Vec<_>>();
    wide.push(0);
    let bytes_len = wide.len() * size_of::<u16>();

    unsafe {
        let handle = GlobalAlloc(GMEM_MOVEABLE, bytes_len);
        if handle == 0 {
            return Err(last_clipboard_error(
                "Failed to allocate clipboard text buffer.",
            ));
        }

        let ptr = GlobalLock(handle) as *mut u16;
        if ptr.is_null() {
            let _ = GlobalFree(handle);
            return Err(last_clipboard_error(
                "Failed to lock clipboard text buffer.",
            ));
        }

        copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
        let _ = GlobalUnlock(handle);

        if SetClipboardData(CF_UNICODETEXT, handle) == 0 {
            let _ = GlobalFree(handle);
            return Err(last_clipboard_error(
                "Failed to publish text to the Windows clipboard.",
            ));
        }
    }

    Ok(())
}

fn write_file_drop_list(paths: &[PathBuf]) -> AppResult<()> {
    let mut encoded_paths = Vec::<u16>::new();
    for path in paths {
        let value = path.to_string_lossy();
        if value.trim().is_empty() {
            continue;
        }
        encoded_paths.extend(value.encode_utf16());
        encoded_paths.push(0);
    }
    encoded_paths.push(0);

    if encoded_paths.len() <= 1 {
        return Err(AppError::Clipboard(
            "No readable file paths were available for the Windows clipboard.".to_string(),
        ));
    }

    let header_size = size_of::<DROPFILES>();
    let bytes_len = header_size + encoded_paths.len() * size_of::<u16>();

    unsafe {
        let handle = GlobalAlloc(GMEM_MOVEABLE, bytes_len);
        if handle == 0 {
            return Err(last_clipboard_error(
                "Failed to allocate clipboard file buffer.",
            ));
        }

        let ptr = GlobalLock(handle) as *mut u8;
        if ptr.is_null() {
            let _ = GlobalFree(handle);
            return Err(last_clipboard_error(
                "Failed to lock clipboard file buffer.",
            ));
        }

        let header = DROPFILES {
            pFiles: header_size as u32,
            pt: POINT { x: 0, y: 0 },
            fNC: 0,
            fWide: 1,
        };
        (ptr as *mut DROPFILES).write(header);
        copy_nonoverlapping(
            encoded_paths.as_ptr() as *const u8,
            ptr.add(header_size),
            encoded_paths.len() * size_of::<u16>(),
        );
        let _ = GlobalUnlock(handle);

        if SetClipboardData(CF_HDROP_FORMAT, handle) == 0 {
            let _ = GlobalFree(handle);
            return Err(last_clipboard_error(
                "Failed to publish files to the Windows clipboard.",
            ));
        }
    }

    Ok(())
}

fn last_clipboard_error(context: &str) -> AppError {
    AppError::Clipboard(format!("{context} {}", std::io::Error::last_os_error()))
}
