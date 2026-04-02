use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
    time::Duration,
};

use jni::{
    JNIEnv,
    objects::{JByteArray, JClass, JString},
    sys::{jboolean, jstring},
};
use parking_lot::Mutex;
use serde::Serialize;
use uuid::Uuid;

use crate::{
    constants::app::STORAGE_DIR_NAME,
    controller::{
        AppController, ControllerRuntimePolicy, HistoryActivation, HistoryRow, HistoryScope,
        HistoryScopeOption, PendingTrustRequest, SettingsSnapshot, SettingsUpdate,
        TransferProgressSnapshot,
    },
    core::{
        at_rest::LocalDataCipher,
        clipboard::{ClipboardBackend, build_item_from_paths, build_item_from_text},
        error::{AppError, AppResult},
        model::{ClipboardItem, ClipboardPayload},
        paths::AppPaths,
    },
    platform::PlatformResult,
};

static ANDROID_APP: OnceLock<AndroidBridge> = OnceLock::new();

pub(crate) fn create_local_data_cipher(_paths: &AppPaths) -> AppResult<LocalDataCipher> {
    Err(AppError::InvalidConfig(
        "Android local-data encryption must be bootstrapped through the native bridge.".to_string(),
    ))
}

pub(crate) fn history_store_profile() -> crate::core::storage::HistoryStoreProfile {
    crate::core::storage::HistoryStoreProfile {
        cache_size_kib: 128,
        full_sync: true,
    }
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
    history_scopes: Vec<HistoryScopeOption>,
    selected_history_scope: String,
    settings: Option<SettingsSnapshot>,
    transfer_progress: Option<AndroidTransferProgress>,
    pending_clipboard: Option<AndroidClipboardWrite>,
    pending_trust_request: Option<PendingTrustRequest>,
    error: Option<String>,
}

#[derive(Serialize)]
struct AndroidActionResult {
    pending_clipboard: Option<AndroidClipboardWrite>,
    transfer_pending: bool,
    transfer_progress: Option<AndroidTransferProgress>,
    error: Option<String>,
}

#[derive(Serialize)]
struct AndroidTickResult {
    pending_clipboard: Option<AndroidClipboardWrite>,
    history_changed: bool,
    devices_changed: bool,
    status_changed: bool,
    transfer_changed: bool,
    transfer_progress: Option<AndroidTransferProgress>,
    paste_requested: bool,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct AndroidTransferProgress {
    item_id: String,
    label: String,
    detail: String,
    fraction: f64,
    source_device_name: String,
    summary: String,
    bytes_done: u64,
    bytes_total: u64,
}

#[derive(Debug, Clone, Serialize)]
struct AndroidClipboardWrite {
    kind: &'static str,
    text: Option<String>,
    paths: Vec<String>,
    item_id: Option<String>,
    summary: Option<String>,
    source_device_name: Option<String>,
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
                item_id: Some(item.id.to_string()),
                summary: Some(item.summary.clone()),
                source_device_name: item.source_device_name.clone(),
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
                Self {
                    kind: "files",
                    text: Some(item.as_clipboard_text()),
                    paths,
                    item_id: Some(item.id.to_string()),
                    summary: Some(item.summary.clone()),
                    source_device_name: item.source_device_name.clone(),
                }
            }
        }
    }
}

impl From<TransferProgressSnapshot> for AndroidTransferProgress {
    fn from(value: TransferProgressSnapshot) -> Self {
        Self {
            item_id: value.item_id.to_string(),
            label: value.label,
            detail: value.detail,
            fraction: value.fraction,
            source_device_name: value.source_device_name,
            summary: value.summary,
            bytes_done: value.bytes_done,
            bytes_total: value.bytes_total,
        }
    }
}

impl AndroidBridge {
    fn bootstrap(
        files_dir: String,
        device_name_hint: Option<String>,
        local_data_key: Vec<u8>,
    ) -> Result<(), String> {
        if ANDROID_APP.get().is_some() {
            return Ok(());
        }

        let root = PathBuf::from(files_dir).join(STORAGE_DIR_NAME);
        let paths = AppPaths::from_root(root);
        paths.ensure().map_err(|error| error.to_string())?;
        let local_data_cipher =
            LocalDataCipher::from_slice(&local_data_key).map_err(|error| error.to_string())?;
        let clipboard = Arc::new(AndroidClipboardState::default());
        let controller = bootstrap_android_controller(
            &paths,
            &clipboard,
            local_data_cipher.clone(),
            device_name_hint.as_deref(),
        )
        .map_err(|error| error.to_string())?;

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
    local_data_cipher: LocalDataCipher,
    device_name_hint: Option<&str>,
) -> AppResult<AppController> {
    AppController::bootstrap(
        paths.clone(),
        Box::new(AndroidClipboardBackend::new(clipboard.clone())),
        local_data_cipher,
        history_store_profile(),
        ControllerRuntimePolicy {
            keep_discovery_running_without_trusted_peers: true,
        },
        device_name_hint,
    )
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

fn jbyte_array_arg(env: &mut JNIEnv<'_>, value: JByteArray<'_>) -> Result<Vec<u8>, String> {
    env.convert_byte_array(value)
        .map_err(|error| error.to_string())
}

fn json_string_list_arg(value: &str) -> Result<Vec<String>, String> {
    serde_json::from_str::<Vec<String>>(value).map_err(|error| error.to_string())
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
    local_data_key: JByteArray<'_>,
) -> jstring {
    let files_dir = match jstring_arg(&mut env, files_dir) {
        Ok(value) => value,
        Err(error) => return to_jstring(&mut env, &error),
    };
    let device_name_hint = match jstring_arg(&mut env, device_name_hint) {
        Ok(value) => optional_text(value),
        Err(error) => return to_jstring(&mut env, &error),
    };
    let local_data_key = match jbyte_array_arg(&mut env, local_data_key) {
        Ok(value) => value,
        Err(error) => return to_jstring(&mut env, &error),
    };

    match AndroidBridge::bootstrap(files_dir, device_name_hint, local_data_key) {
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
        let text = text.trim();
        if text.is_empty() {
            return Ok(());
        }
        let (device_id, device_name) = {
            let controller = bridge.controller.lock();
            controller.local_device_identity()
        };
        let item = build_item_from_text(&text, Some(device_id), Some(device_name), false);
        bridge.controller.lock().submit_local_clipboard_item(item);
        Ok(())
    }) {
        Ok(()) => null_jstring(),
        Err(error) => to_jstring(&mut env, &error),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_benfach_cliplink_RustBridge_nativeSubmitClipboardFiles(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    paths_json: JString<'_>,
) -> jstring {
    let paths_json = match jstring_arg(&mut env, paths_json) {
        Ok(value) => value,
        Err(error) => return to_jstring(&mut env, &error),
    };

    match with_bridge(|bridge| {
        let raw_paths = json_string_list_arg(paths_json.trim())?;
        let paths = raw_paths
            .into_iter()
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .collect::<Vec<_>>();
        if paths.is_empty() {
            return Ok(());
        }

        let (device_id, device_name) = {
            let controller = bridge.controller.lock();
            controller.local_device_identity()
        };
        let Some(item) = build_item_from_paths(&paths, Some(device_id), Some(device_name), false)
        else {
            return Err("Clipboard file paths are no longer available.".to_string());
        };
        bridge.controller.lock().submit_local_clipboard_item(item);
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
    scope: JString<'_>,
) -> jstring {
    let query = jstring_arg(&mut env, query).unwrap_or_default();
    let scope = jstring_arg(&mut env, scope).unwrap_or_default();
    let scope = HistoryScope::from_raw(&scope);

    let payload = match with_bridge(|bridge| {
        let controller = bridge.controller.lock();
        let normalized_scope = controller.normalize_history_scope(scope);
        Ok(AndroidSnapshot {
            history: controller.history_rows_with_scope(&query, normalized_scope.clone()),
            history_scopes: controller.history_scope_options(),
            selected_history_scope: normalized_scope.key(),
            settings: Some(controller.settings_snapshot()),
            transfer_progress: controller
                .transfer_progress_snapshot()
                .map(AndroidTransferProgress::from),
            pending_clipboard: bridge.clipboard.take_pending_write(),
            pending_trust_request: controller.pending_trust_request(),
            error: None,
        })
    }) {
        Ok(snapshot) => snapshot,
        Err(error) => AndroidSnapshot {
            history: Vec::new(),
            history_scopes: Vec::new(),
            selected_history_scope: HistoryScope::All.key(),
            settings: None,
            transfer_progress: None,
            pending_clipboard: None,
            pending_trust_request: None,
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
            transfer_changed: outcome.transfer_changed,
            transfer_progress: controller
                .transfer_progress_snapshot()
                .map(AndroidTransferProgress::from),
            paste_requested: outcome.paste_requested,
            error: None,
        })
    }) {
        Ok(result) => result,
        Err(error) => AndroidTickResult {
            pending_clipboard: None,
            history_changed: false,
            devices_changed: false,
            status_changed: false,
            transfer_changed: false,
            transfer_progress: None,
            paste_requested: false,
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
                    transfer_pending: false,
                    transfer_progress: None,
                    error: Some(error),
                }),
            );
        }
    };

    let result = match with_bridge(|bridge| {
        let id = Uuid::parse_str(item_id.trim()).map_err(|error| error.to_string())?;
        let mut controller = bridge.controller.lock();
        let activation = controller
            .copy_item(id)
            .map_err(|error| error.to_string())?;
        Ok(AndroidActionResult {
            transfer_progress: controller
                .transfer_progress_snapshot()
                .map(AndroidTransferProgress::from),
            pending_clipboard: matches!(activation, HistoryActivation::ClipboardReady)
                .then(|| bridge.clipboard.take_pending_write())
                .flatten(),
            transfer_pending: matches!(activation, HistoryActivation::PendingTransfer),
            error: matches!(activation, HistoryActivation::Noop)
                .then_some("History item was not found.".to_string()),
        })
    }) {
        Ok(result) => result,
        Err(error) => AndroidActionResult {
            pending_clipboard: None,
            transfer_pending: false,
            transfer_progress: None,
            error: Some(error),
        },
    };

    to_jstring(&mut env, &serialize_json(&result))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_benfach_cliplink_RustBridge_nativeDeleteHistoryItem(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    item_id: JString<'_>,
) -> jstring {
    let item_id = match jstring_arg(&mut env, item_id) {
        Ok(value) => value,
        Err(error) => return to_jstring(&mut env, &error),
    };

    match with_bridge(|bridge| {
        let id = Uuid::parse_str(item_id.trim()).map_err(|error| error.to_string())?;
        let deleted = bridge
            .controller
            .lock()
            .delete_history_item(id)
            .map_err(|error| error.to_string())?;
        if deleted {
            Ok(())
        } else {
            Err("History item was not found.".to_string())
        }
    }) {
        Ok(()) => null_jstring(),
        Err(error) => to_jstring(&mut env, &error),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_benfach_cliplink_RustBridge_nativeDeleteHistoryItems(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    item_ids_json: JString<'_>,
) -> jstring {
    let item_ids_json = match jstring_arg(&mut env, item_ids_json) {
        Ok(value) => value,
        Err(error) => return to_jstring(&mut env, &error),
    };

    match with_bridge(|bridge| {
        let ids = json_string_list_arg(item_ids_json.trim())?
            .into_iter()
            .map(|value| Uuid::parse_str(value.trim()).map_err(|error| error.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        let deleted = bridge
            .controller
            .lock()
            .delete_history_items(&ids)
            .map_err(|error| error.to_string())?;
        if deleted > 0 {
            Ok(())
        } else {
            Err("No editable local history items were selected.".to_string())
        }
    }) {
        Ok(()) => null_jstring(),
        Err(error) => to_jstring(&mut env, &error),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_benfach_cliplink_RustBridge_nativeToggleHistoryItemPin(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    item_id: JString<'_>,
) -> jstring {
    let item_id = match jstring_arg(&mut env, item_id) {
        Ok(value) => value,
        Err(error) => return to_jstring(&mut env, &error),
    };

    match with_bridge(|bridge| {
        let id = Uuid::parse_str(item_id.trim()).map_err(|error| error.to_string())?;
        let toggled = bridge
            .controller
            .lock()
            .toggle_history_item_pin(id)
            .map_err(|error| error.to_string())?;
        if toggled.is_some() {
            Ok(())
        } else {
            Err("History item was not found.".to_string())
        }
    }) {
        Ok(()) => null_jstring(),
        Err(error) => to_jstring(&mut env, &error),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_benfach_cliplink_RustBridge_nativeSetHistoryItemsPinned(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    item_ids_json: JString<'_>,
    pinned: jboolean,
) -> jstring {
    let item_ids_json = match jstring_arg(&mut env, item_ids_json) {
        Ok(value) => value,
        Err(error) => return to_jstring(&mut env, &error),
    };

    match with_bridge(|bridge| {
        let ids = json_string_list_arg(item_ids_json.trim())?
            .into_iter()
            .map(|value| Uuid::parse_str(value.trim()).map_err(|error| error.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        let changed = bridge
            .controller
            .lock()
            .set_history_items_pinned(&ids, pinned != 0)
            .map_err(|error| error.to_string())?;
        if changed > 0 {
            Ok(())
        } else {
            Err("No editable local history items were selected.".to_string())
        }
    }) {
        Ok(()) => null_jstring(),
        Err(error) => to_jstring(&mut env, &error),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_benfach_cliplink_RustBridge_nativeClearHistory(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    include_pinned: jboolean,
) -> jstring {
    match with_bridge(|bridge| {
        bridge
            .controller
            .lock()
            .clear_history_with_options(include_pinned != 0)
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
        let requested = bridge
            .controller
            .lock()
            .request_device_trust(device_id.trim())
            .map_err(|error| error.to_string())?;
        if requested {
            Ok(())
        } else {
            Err("The selected device is not currently available for trust.".to_string())
        }
    }) {
        Ok(()) => null_jstring(),
        Err(error) => to_jstring(&mut env, &error),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_benfach_cliplink_RustBridge_nativeTrustDevices(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    device_ids_json: JString<'_>,
) -> jstring {
    let device_ids_json = match jstring_arg(&mut env, device_ids_json) {
        Ok(value) => value,
        Err(error) => return to_jstring(&mut env, &error),
    };

    match with_bridge(|bridge| {
        let device_ids = json_string_list_arg(device_ids_json.trim())?;
        let requested = bridge
            .controller
            .lock()
            .request_device_trust_many(&device_ids)
            .map_err(|error| error.to_string())?;
        if requested > 0 {
            Ok(())
        } else {
            Err("No currently available devices were selected for trust.".to_string())
        }
    }) {
        Ok(()) => null_jstring(),
        Err(error) => to_jstring(&mut env, &error),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_benfach_cliplink_RustBridge_nativeRespondTrustRequest(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    request_id: JString<'_>,
    allow: bool,
) -> jstring {
    let request_id = match jstring_arg(&mut env, request_id) {
        Ok(value) => value,
        Err(error) => return to_jstring(&mut env, &error),
    };

    match with_bridge(|bridge| {
        let request_id = Uuid::parse_str(request_id.trim()).map_err(|error| error.to_string())?;
        let handled = bridge
            .controller
            .lock()
            .respond_to_trust_request(request_id, allow)
            .map_err(|error| error.to_string())?;
        if handled {
            Ok(())
        } else {
            Err("The trust request is no longer pending.".to_string())
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

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_benfach_cliplink_RustBridge_nativeRevokeTrustedDevices(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    device_ids_json: JString<'_>,
) -> jstring {
    let device_ids_json = match jstring_arg(&mut env, device_ids_json) {
        Ok(value) => value,
        Err(error) => return to_jstring(&mut env, &error),
    };

    match with_bridge(|bridge| {
        let device_ids = json_string_list_arg(device_ids_json.trim())?;
        let revoked = bridge
            .controller
            .lock()
            .revoke_device_trust_many(&device_ids)
            .map_err(|error| error.to_string())?;
        if revoked > 0 {
            Ok(())
        } else {
            Err("The selected devices are not trusted.".to_string())
        }
    }) {
        Ok(()) => null_jstring(),
        Err(error) => to_jstring(&mut env, &error),
    }
}

pub(super) fn run(_controller: AppController) -> PlatformResult {
    Err("Android uses the native Activity bridge entrypoint instead of platform::run.".into())
}
