use std::fs::{self, OpenOptions};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use ring::rand::{SecureRandom, SystemRandom};

use crate::{
    constants::crypto::LOCAL_DATA_KEY_BYTES,
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
    if paths.local_data_key_file.exists() {
        let key_bytes = fs::read(&paths.local_data_key_file)?;
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

    let temp_path = paths.local_data_key_file.with_extension("tmp");
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temp_path)?;
    restrict_owner_permissions(&temp_path)?;
    std::io::Write::write_all(&mut file, &key_bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temp_path, &paths.local_data_key_file)?;
    restrict_owner_permissions(&paths.local_data_key_file)?;
    sync_parent_dir(&paths.local_data_key_file)?;

    Ok(key_bytes)
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
