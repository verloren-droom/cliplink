use std::fs::{self, OpenOptions};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use ring::rand::{SecureRandom, SystemRandom};

use crate::{
    constants::crypto::{LOCAL_DATA_KEY_BYTES, LOCAL_DATA_KEY_FILE_NAME},
    core::{
        at_rest::{LocalDataCipher, sync_parent_dir},
        error::{AppError, AppResult},
        paths::AppPaths,
    },
};

/// File-backed local data key provider used on platforms without native key storage yet.
pub(super) fn create_local_data_cipher(paths: &AppPaths) -> AppResult<LocalDataCipher> {
    let key_bytes = load_or_create_local_data_key(paths)?;
    LocalDataCipher::from_slice(&key_bytes)
}

fn load_or_create_local_data_key(paths: &AppPaths) -> AppResult<[u8; LOCAL_DATA_KEY_BYTES]> {
    let key_path = local_data_key_path(paths);

    if key_path.exists() {
        let key_bytes = fs::read(&key_path)?;
        return key_bytes.try_into().map_err(|_| {
            AppError::Crypto(format!(
                "Local data key file must be exactly {LOCAL_DATA_KEY_BYTES} bytes."
            ))
        });
    }

    let mut key_bytes = [0_u8; LOCAL_DATA_KEY_BYTES];
    SystemRandom::new()
        .fill(&mut key_bytes)
        .map_err(|_| AppError::Crypto("Failed to generate a local data key.".to_string()))?;

    let temp_path = key_path.with_extension("tmp");
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temp_path)?;
    restrict_owner_permissions(&temp_path)?;
    std::io::Write::write_all(&mut file, &key_bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temp_path, &key_path)?;
    restrict_owner_permissions(&key_path)?;
    sync_parent_dir(&key_path)?;

    Ok(key_bytes)
}

fn local_data_key_path(paths: &AppPaths) -> std::path::PathBuf {
    paths.root.join(LOCAL_DATA_KEY_FILE_NAME)
}

fn restrict_owner_permissions(path: &std::path::Path) -> AppResult<()> {
    #[cfg(unix)]
    {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }

    #[cfg(not(unix))]
    {
        let _ = path;
    }

    Ok(())
}
