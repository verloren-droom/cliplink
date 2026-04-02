use crate::{
    controller::{AppController, ControllerRuntimePolicy},
    core::{
        at_rest::LocalDataCipher, clipboard::ClipboardBackend, error::AppResult, paths::AppPaths,
        storage::HistoryStoreProfile,
    },
    platform::{self, PlatformResult},
};

pub(crate) fn controller_runtime_policy() -> ControllerRuntimePolicy {
    ControllerRuntimePolicy::default()
}

pub(crate) fn device_name_hint() -> Option<String> {
    platform::host_identity::device_name_hint()
}

pub(crate) fn create_clipboard_backend() -> AppResult<Box<dyn ClipboardBackend>> {
    platform::system::create_clipboard_backend()
}

pub(crate) fn discover_app_paths() -> AppResult<AppPaths> {
    platform::host_paths::discover_app_paths()
}

pub(crate) fn history_store_profile() -> HistoryStoreProfile {
    HistoryStoreProfile::default()
}

pub(crate) fn create_local_data_cipher(paths: &AppPaths) -> AppResult<LocalDataCipher> {
    platform::keyfile::create_local_data_cipher(paths)
}

pub(crate) fn run(controller: AppController) -> PlatformResult {
    let _ = controller;
    Err("The native UI is currently implemented only on macOS, Windows, and Android.".into())
}
