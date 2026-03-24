use std::{
    fs::{File, OpenOptions},
    sync::OnceLock,
};

use fs2::FileExt;
use thiserror::Error;

use crate::{
    controller::AppController,
    core::{
        error::{AppError, AppResult},
        paths::AppPaths,
    },
    platform,
};

static NATIVE_UI_INSTANCE_LOCK: OnceLock<File> = OnceLock::new();

#[derive(Debug, Error)]
enum StartupError {
    #[error("failed to validate launch arguments: {source}")]
    ValidateLaunchArgs { source: AppError },
    #[error("failed to discover application paths: {source}")]
    DiscoverPaths { source: AppError },
    #[error("failed to claim the single-instance UI lock: {source}")]
    ClaimInstanceLock { source: AppError },
    #[error("failed to initialize the local data cipher: {source}")]
    CreateLocalDataCipher { source: AppError },
    #[error("failed to initialize the clipboard backend: {source}")]
    CreateClipboardBackend { source: AppError },
    #[error("failed to bootstrap the application controller: {source}")]
    BootstrapController { source: AppError },
}

pub fn run_native_ui_app() -> Result<(), Box<dyn std::error::Error>> {
    let device_name_hint = platform::device_name_hint();

    validate_native_ui_launch_args()
        .map_err(|source| StartupError::ValidateLaunchArgs { source })?;

    let paths =
        platform::discover_app_paths().map_err(|source| StartupError::DiscoverPaths { source })?;
    if !claim_native_ui_instance(&paths)
        .map_err(|source| StartupError::ClaimInstanceLock { source })?
    {
        return Ok(());
    }

    let local_data_cipher = platform::create_local_data_cipher(&paths)
        .map_err(|source| StartupError::CreateLocalDataCipher { source })?;
    let clipboard = platform::create_clipboard_backend()
        .map_err(|source| StartupError::CreateClipboardBackend { source })?;
    let controller = AppController::bootstrap(
        paths,
        clipboard,
        local_data_cipher,
        device_name_hint.as_deref(),
    )
    .map_err(|source| StartupError::BootstrapController { source })?;
    platform::run(controller)
}

fn validate_native_ui_launch_args() -> AppResult<()> {
    for arg in std::env::args_os().skip(1) {
        let value = arg.to_string_lossy();
        if value == "ui" || value.starts_with("-psn_") {
            continue;
        }

        return Err(AppError::InvalidConfig(format!(
            "Unsupported argument `{value}`. This build only supports the native UI launch path."
        )));
    }

    Ok(())
}

fn claim_native_ui_instance(paths: &AppPaths) -> AppResult<bool> {
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(paths.root.join("ui.lock"))?;

    match file.try_lock_exclusive() {
        Ok(()) => {
            let _ = NATIVE_UI_INSTANCE_LOCK.set(file);
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(false),
        Err(error) => Err(error.into()),
    }
}
