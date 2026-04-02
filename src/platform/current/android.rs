use crate::{
    controller::{AppController, ControllerRuntimePolicy},
    core::{
        at_rest::LocalDataCipher,
        clipboard::ClipboardBackend,
        error::{AppError, AppResult},
        paths::AppPaths,
    },
    platform::{self, PlatformResult},
};

pub(crate) fn controller_runtime_policy() -> ControllerRuntimePolicy {
    ControllerRuntimePolicy::default()
}

pub(crate) fn device_name_hint() -> Option<String> {
    None
}

pub(crate) fn create_clipboard_backend() -> AppResult<Box<dyn ClipboardBackend>> {
    Err(AppError::InvalidConfig(
        "Android uses the native bridge bootstrap instead of the desktop clipboard backend."
            .to_string(),
    ))
}

pub(crate) fn discover_app_paths() -> AppResult<AppPaths> {
    Err(AppError::InvalidConfig(
        "Android paths must be provided by the native bridge.".to_string(),
    ))
}

pub(crate) fn history_store_profile() -> crate::core::storage::HistoryStoreProfile {
    platform::android::history_store_profile()
}

pub(crate) fn create_local_data_cipher(paths: &AppPaths) -> AppResult<LocalDataCipher> {
    platform::android::create_local_data_cipher(paths)
}

pub(crate) fn run(controller: AppController) -> PlatformResult {
    platform::android::run(controller)
}
