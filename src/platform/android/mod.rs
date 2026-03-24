use std::{
    ffi::OsString,
    fs,
    path::PathBuf,
    sync::{Arc, OnceLock},
    time::Duration,
};

use jni::{
    JNIEnv,
    objects::{JClass, JString},
    sys::jstring,
};
use parking_lot::Mutex;
use serde::Serialize;
use uuid::Uuid;

use crate::{
    constants::app::STORAGE_DIR_NAME,
    controller::{AppController, HistoryRow, SettingsSnapshot, SettingsUpdate},
    core::{
        at_rest::LocalDataCipher,
        clipboard::ClipboardBackend,
        error::{AppError, AppResult},
        model::{ClipboardItem, ClipboardPayload},
        paths::AppPaths,
    },
    platform::PlatformResult,
};

static ANDROID_APP: OnceLock<AndroidBridge> = OnceLock::new();

pub(crate) fn create_local_data_cipher(paths: &AppPaths) -> AppResult<LocalDataCipher> {
    super::keyfile::create_local_data_cipher(paths)
}

/// Android bridge state shared by the JNI entry points and the core controller.
struct AndroidBridge {
    controller: Mutex<AppController>,
    clipboard: Arc<AndroidClipboardState>,
}

/// Android clipboard state bridged through the native Activity instead of direct Rust OS calls.
#[derive(Default)]
struct AndroidClipboardState {
    pending_write: Mutex<Option<AndroidClipboardWrite>>,
}

/// Android clipboard backend used by the controller on mobile targets.
struct AndroidClipboardBackend {
    state: Arc<AndroidClipboardState>,
}

#[derive(Serialize)]
struct AndroidSnapshot {
    history: Vec<HistoryRow>,
    settings: Option<SettingsSnapshot>,
    pending_clipboard: Option<AndroidClipboardWrite>,
    error: Option<String>,
}

#[derive(Serialize)]
struct AndroidActionResult {
    pending_clipboard: Option<AndroidClipboardWrite>,
    error: Option<String>,
}

#[derive(Serialize)]
struct AndroidTickResult {
    pending_clipboard: Option<AndroidClipboardWrite>,
    history_changed: bool,
    devices_changed: bool,
    status_changed: bool,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct AndroidClipboardWrite {
    kind: &'static str,
    text: Option<String>,
    paths: Vec<String>,
}

impl AndroidClipboardState {
    fn store_pending_write(&self, write: AndroidClipboardWrite) {
        *self.pending_write.lock() = Some(write);
    }

    fn take_pending_write(&self) -> Option<AndroidClipboardWrite> {
        self.pending_write.lock().take()
    }
}

impl AndroidClipboardBackend {
    fn new(state: Arc<AndroidClipboardState>) -> Self {
        Self { state }
    }
}

impl ClipboardBackend for AndroidClipboardBackend {
    fn recommended_poll_interval(&self) -> Duration {
        Duration::MAX
    }

    fn poll(
        &self,
        _source_device_id: &str,
        _source_device_name: &str,
    ) -> AppResult<Option<ClipboardItem>> {
        Ok(None)
    }

    fn write_item(&self, item: &ClipboardItem) -> AppResult<()> {
        self.state
            .store_pending_write(AndroidClipboardWrite::from_item(item));
        Ok(())
    }
}

impl AndroidClipboardWrite {
    fn from_item(item: &ClipboardItem) -> Self {
        match &item.payload {
            ClipboardPayload::Text(text) => Self {
                kind: "text",
                text: Some(text.clone()),
                paths: Vec::new(),
            },
            ClipboardPayload::Files(files) => {
                let paths = files
                    .iter()
                    .filter_map(|file| {
                        file.local_path
                            .as_ref()
                            .or(file.source_path.as_ref())
                            .map(|path| path.to_string_lossy().to_string())
                    })
                    .collect::<Vec<_>>();

                if paths.is_empty() {
                    Self {
                        kind: "text",
                        text: Some(item.as_clipboard_text()),
                        paths: Vec::new(),
                    }
                } else {
                    Self {
                        kind: "files",
                        text: Some(item.as_clipboard_text()),
                        paths,
                    }
                }
            }
        }
    }
}

impl AndroidBridge {
    fn bootstrap(files_dir: String, device_name_hint: Option<String>) -> Result<(), String> {
        if ANDROID_APP.get().is_some() {
            return Ok(());
        }

        let root = PathBuf::from(files_dir).join(STORAGE_DIR_NAME);
        let paths = AppPaths::from_root(root);
        paths.ensure().map_err(|error| error.to_string())?;
        let clipboard = Arc::new(AndroidClipboardState::default());
        let (controller, recovered_state) =
            bootstrap_android_controller(&paths, &clipboard, device_name_hint.as_deref())
                .map_err(|error| error.to_string())?;
        let mut controller = controller;
        if recovered_state {
            controller.report_status("检测到旧的或损坏的本地数据，已自动重建 Android 本地存储");
        }

        let bridge = AndroidBridge {
            controller: Mutex::new(controller),
            clipboard,
        };
        let _ = ANDROID_APP.set(bridge);
        Ok(())
    }
}

fn bootstrap_android_controller(
    paths: &AppPaths,
    clipboard: &Arc<AndroidClipboardState>,
    device_name_hint: Option<&str>,
) -> AppResult<(AppController, bool)> {
    match try_bootstrap_android_controller(paths, clipboard, device_name_hint) {
        Ok(controller) => Ok((controller, false)),
        Err(error) if is_recoverable_persistent_state_error(&error) => {
            reset_android_persistent_state(paths)?;
            let controller = try_bootstrap_android_controller(paths, clipboard, device_name_hint)?;
            Ok((controller, true))
        }
        Err(error) => Err(error),
    }
}

fn try_bootstrap_android_controller(
    paths: &AppPaths,
    clipboard: &Arc<AndroidClipboardState>,
    device_name_hint: Option<&str>,
) -> AppResult<AppController> {
    let local_data_cipher = create_local_data_cipher(paths)?;
    AppController::bootstrap(
        paths.clone(),
        Box::new(AndroidClipboardBackend::new(clipboard.clone())),
        local_data_cipher,
        device_name_hint,
    )
}

fn is_recoverable_persistent_state_error(error: &AppError) -> bool {
    matches!(
        error,
        AppError::Crypto(_) | AppError::Json(_) | AppError::Sql(_) | AppError::InvalidConfig(_)
    )
}

fn reset_android_persistent_state(paths: &AppPaths) -> AppResult<()> {
    let mut files_to_remove = vec![
        paths.config_file.clone(),
        paths.history_db.clone(),
        append_path_suffix(&paths.history_db, "-shm"),
        append_path_suffix(&paths.history_db, "-wal"),
        paths.cert_der.clone(),
        paths.key_der.clone(),
    ];
    #[cfg(not(target_os = "macos"))]
    files_to_remove.push(paths.local_data_key_file.clone());

    for path in files_to_remove {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }

    Ok(())
}

fn append_path_suffix(path: &std::path::Path, suffix: &str) -> PathBuf {
    let mut value = OsString::from(path.as_os_str());
    value.push(suffix);
    PathBuf::from(value)
}

fn with_bridge<T>(f: impl FnOnce(&AndroidBridge) -> Result<T, String>) -> Result<T, String> {
    let bridge = ANDROID_APP
        .get()
        .ok_or_else(|| "Android bridge has not been initialized.".to_string())?;
    f(bridge)
}

fn jstring_arg(env: &mut JNIEnv<'_>, value: JString<'_>) -> Result<String, String> {
    env.get_string(&value)
        .map(|text| text.to_string_lossy().into_owned())
        .map_err(|error| error.to_string())
}

fn optional_text(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn to_jstring(env: &mut JNIEnv<'_>, value: &str) -> jstring {
    match env.new_string(value) {
        Ok(text) => text.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

fn null_jstring() -> jstring {
    std::ptr::null_mut()
}

fn serialize_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|error| {
        serde_json::json!({
            "history": [],
            "settings": null,
            "pending_clipboard": null,
            "error": error.to_string(),
        })
        .to_string()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_benfach_cliplink_RustBridge_nativeBootstrap(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    files_dir: JString<'_>,
    device_name_hint: JString<'_>,
) -> jstring {
    let files_dir = match jstring_arg(&mut env, files_dir) {
        Ok(value) => value,
        Err(error) => return to_jstring(&mut env, &error),
    };
    let device_name_hint = match jstring_arg(&mut env, device_name_hint) {
        Ok(value) => optional_text(value),
        Err(error) => return to_jstring(&mut env, &error),
    };

    match AndroidBridge::bootstrap(files_dir, device_name_hint) {
        Ok(()) => null_jstring(),
        Err(error) => to_jstring(&mut env, &error),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_benfach_cliplink_RustBridge_nativeSubmitClipboardText(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    text: JString<'_>,
) -> jstring {
    let text = match jstring_arg(&mut env, text) {
        Ok(value) => value,
        Err(error) => return to_jstring(&mut env, &error),
    };

    match with_bridge(|bridge| {
        bridge.controller.lock().submit_local_clipboard_text(&text);
        Ok(())
    }) {
        Ok(()) => null_jstring(),
        Err(error) => to_jstring(&mut env, &error),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_benfach_cliplink_RustBridge_nativeRefreshSnapshot(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    query: JString<'_>,
) -> jstring {
    let query = jstring_arg(&mut env, query).unwrap_or_default();

    let payload = match with_bridge(|bridge| {
        let mut controller = bridge.controller.lock();
        controller.tick();
        Ok(AndroidSnapshot {
            history: controller.history_rows(&query),
            settings: Some(controller.settings_snapshot()),
            pending_clipboard: bridge.clipboard.take_pending_write(),
            error: None,
        })
    }) {
        Ok(snapshot) => snapshot,
        Err(error) => AndroidSnapshot {
            history: Vec::new(),
            settings: None,
            pending_clipboard: None,
            error: Some(error),
        },
    };

    to_jstring(&mut env, &serialize_json(&payload))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_benfach_cliplink_RustBridge_nativeTick(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
) -> jstring {
    let payload = match with_bridge(|bridge| {
        let mut controller = bridge.controller.lock();
        let outcome = controller.tick();
        Ok(AndroidTickResult {
            pending_clipboard: bridge.clipboard.take_pending_write(),
            history_changed: outcome.history_changed,
            devices_changed: outcome.devices_changed,
            status_changed: outcome.status_changed,
            error: None,
        })
    }) {
        Ok(result) => result,
        Err(error) => AndroidTickResult {
            pending_clipboard: None,
            history_changed: false,
            devices_changed: false,
            status_changed: false,
            error: Some(error),
        },
    };

    to_jstring(&mut env, &serialize_json(&payload))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_benfach_cliplink_RustBridge_nativeActivateHistoryItem(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    item_id: JString<'_>,
) -> jstring {
    let item_id = match jstring_arg(&mut env, item_id) {
        Ok(value) => value,
        Err(error) => {
            return to_jstring(
                &mut env,
                &serialize_json(&AndroidActionResult {
                    pending_clipboard: None,
                    error: Some(error),
                }),
            );
        }
    };

    let result = match with_bridge(|bridge| {
        let id = Uuid::parse_str(item_id.trim()).map_err(|error| error.to_string())?;
        let copied = bridge
            .controller
            .lock()
            .copy_item(id)
            .map_err(|error| error.to_string())?;
        if !copied {
            return Err("History item was not found.".to_string());
        }
        Ok(AndroidActionResult {
            pending_clipboard: bridge.clipboard.take_pending_write(),
            error: None,
        })
    }) {
        Ok(result) => result,
        Err(error) => AndroidActionResult {
            pending_clipboard: None,
            error: Some(error),
        },
    };

    to_jstring(&mut env, &serialize_json(&result))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_benfach_cliplink_RustBridge_nativeClearHistory(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
) -> jstring {
    match with_bridge(|bridge| {
        bridge
            .controller
            .lock()
            .clear_history()
            .map_err(|error| error.to_string())
    }) {
        Ok(()) => null_jstring(),
        Err(error) => to_jstring(&mut env, &error),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_benfach_cliplink_RustBridge_nativeSaveSettings(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    settings_json: JString<'_>,
) -> jstring {
    let settings_json = match jstring_arg(&mut env, settings_json) {
        Ok(value) => value,
        Err(error) => return to_jstring(&mut env, &error),
    };

    match with_bridge(|bridge| {
        let update = serde_json::from_str::<SettingsUpdate>(&settings_json)
            .map_err(|error| error.to_string())?;
        bridge
            .controller
            .lock()
            .apply_settings(update)
            .map_err(|error| error.to_string())
    }) {
        Ok(()) => null_jstring(),
        Err(error) => to_jstring(&mut env, &error),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_benfach_cliplink_RustBridge_nativeTrustDevice(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    device_id: JString<'_>,
) -> jstring {
    let device_id = match jstring_arg(&mut env, device_id) {
        Ok(value) => value,
        Err(error) => return to_jstring(&mut env, &error),
    };

    match with_bridge(|bridge| {
        let trusted = bridge
            .controller
            .lock()
            .trust_device(device_id.trim())
            .map_err(|error| error.to_string())?;
        if trusted {
            Ok(())
        } else {
            Err("The selected device is not currently online.".to_string())
        }
    }) {
        Ok(()) => null_jstring(),
        Err(error) => to_jstring(&mut env, &error),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_benfach_cliplink_RustBridge_nativeRevokeTrustedDevice(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    device_id: JString<'_>,
) -> jstring {
    let device_id = match jstring_arg(&mut env, device_id) {
        Ok(value) => value,
        Err(error) => return to_jstring(&mut env, &error),
    };

    match with_bridge(|bridge| {
        let revoked = bridge
            .controller
            .lock()
            .revoke_device_trust(device_id.trim())
            .map_err(|error| error.to_string())?;
        if revoked {
            Ok(())
        } else {
            Err("The selected device is not trusted.".to_string())
        }
    }) {
        Ok(()) => null_jstring(),
        Err(error) => to_jstring(&mut env, &error),
    }
}

pub fn run(_controller: AppController) -> PlatformResult {
    Err("Android uses the native Activity bridge entrypoint instead of platform::run.".into())
}
