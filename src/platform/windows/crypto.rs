use std::fs;

use ring::rand::{SecureRandom, SystemRandom};
use windows_sys::Win32::{
    Security::Cryptography::{
        CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData, DATA_BLOB,
    },
    System::Memory::LocalFree,
};

use crate::{
    constants::{
        app::APP_NAME,
        crypto::{LOCAL_DATA_KEY_BYTES, LOCAL_DATA_KEY_FILE_NAME},
    },
    core::{
        at_rest::LocalDataCipher,
        error::{AppError, AppResult},
        paths::AppPaths,
    },
};

/// Windows DPAPI-backed local key provider.
pub(super) fn create_local_data_cipher(paths: &AppPaths) -> AppResult<LocalDataCipher> {
    let key_bytes = load_or_create_local_data_key(paths)?;
    LocalDataCipher::from_slice(&key_bytes)
}

fn load_or_create_local_data_key(paths: &AppPaths) -> AppResult<[u8; LOCAL_DATA_KEY_BYTES]> {
    if paths.local_data_key_file.exists() {
        let protected = fs::read(&paths.local_data_key_file)?;
        let plaintext = unprotect_bytes(&protected)?;
        return plaintext.try_into().map_err(|_| {
            AppError::Crypto(format!(
                "Protected local data key must be exactly {LOCAL_DATA_KEY_BYTES} bytes."
            ))
        });
    }

    let mut key_bytes = [0_u8; LOCAL_DATA_KEY_BYTES];
    SystemRandom::new()
        .fill(&mut key_bytes)
        .map_err(|_| AppError::Crypto("Failed to generate a local data key.".to_string()))?;

    let protected = protect_bytes(&key_bytes)?;
    let temp_path = paths
        .local_data_key_file
        .with_file_name(format!("{LOCAL_DATA_KEY_FILE_NAME}.tmp"));
    fs::write(&temp_path, protected)?;
    fs::rename(&temp_path, &paths.local_data_key_file)?;

    Ok(key_bytes)
}

fn protect_bytes(plaintext: &[u8]) -> AppResult<Vec<u8>> {
    let mut input = DATA_BLOB {
        cbData: plaintext.len() as u32,
        pbData: plaintext.as_ptr() as *mut u8,
    };
    let description = wide(APP_NAME);
    let mut output = DATA_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };

    unsafe {
        let ok = CryptProtectData(
            &mut input,
            description.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        );
        if ok == 0 {
            return Err(last_crypto_error(
                "Failed to protect the Windows local data key.",
            ));
        }

        let sealed = copy_blob_bytes(&output);
        let _ = LocalFree(output.pbData as isize);
        sealed
    }
}

fn unprotect_bytes(sealed: &[u8]) -> AppResult<Vec<u8>> {
    let mut input = DATA_BLOB {
        cbData: sealed.len() as u32,
        pbData: sealed.as_ptr() as *mut u8,
    };
    let mut output = DATA_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };

    unsafe {
        let ok = CryptUnprotectData(
            &mut input,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        );
        if ok == 0 {
            return Err(last_crypto_error(
                "Failed to unprotect the Windows local data key.",
            ));
        }

        let plaintext = copy_blob_bytes(&output);
        let _ = LocalFree(output.pbData as isize);
        plaintext
    }
}

unsafe fn copy_blob_bytes(blob: &DATA_BLOB) -> AppResult<Vec<u8>> {
    if blob.pbData.is_null() || blob.cbData == 0 {
        return Err(AppError::Crypto(
            "Windows DPAPI returned an empty data blob.".to_string(),
        ));
    }

    Ok(std::slice::from_raw_parts(blob.pbData, blob.cbData as usize).to_vec())
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn last_crypto_error(context: &str) -> AppError {
    AppError::Crypto(format!("{context} {}", std::io::Error::last_os_error()))
}
