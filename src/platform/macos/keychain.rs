use std::{ffi::c_void, ptr};

use ring::rand::{SecureRandom, SystemRandom};

use crate::{
    constants::{
        app::APP_BUNDLE_ID,
        crypto::{LOCAL_DATA_KEY_ACCOUNT, LOCAL_DATA_KEY_BYTES},
    },
    core::{
        at_rest::LocalDataCipher,
        error::{AppError, AppResult},
        paths::AppPaths,
    },
};

type OSStatus = i32;
type SecKeychainItemRef = *mut c_void;

const ERR_SEC_ITEM_NOT_FOUND: OSStatus = -25_300;
const ERR_SEC_DUPLICATE_ITEM: OSStatus = -25_299;

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn SecKeychainAddGenericPassword(
        keychain: *const c_void,
        service_name_length: u32,
        service_name: *const i8,
        account_name_length: u32,
        account_name: *const i8,
        password_length: u32,
        password_data: *const c_void,
        item_ref: *mut SecKeychainItemRef,
    ) -> OSStatus;

    fn SecKeychainFindGenericPassword(
        keychain_or_array: *const c_void,
        service_name_length: u32,
        service_name: *const i8,
        account_name_length: u32,
        account_name: *const i8,
        password_length: *mut u32,
        password_data: *mut *mut c_void,
        item_ref: *mut SecKeychainItemRef,
    ) -> OSStatus;

    fn SecKeychainItemFreeContent(attr_list: *mut c_void, data: *mut c_void) -> OSStatus;
}

/// macOS Keychain-backed local data key provider.
pub(super) fn create_local_data_cipher(_paths: &AppPaths) -> AppResult<LocalDataCipher> {
    let key_bytes = load_or_create_local_data_key()?;
    LocalDataCipher::from_slice(&key_bytes)
}

fn load_or_create_local_data_key() -> AppResult<[u8; LOCAL_DATA_KEY_BYTES]> {
    if let Some(key_bytes) = load_generic_password()? {
        return key_bytes.try_into().map_err(|_| {
            AppError::Crypto(format!(
                "Stored Keychain master key must be exactly {LOCAL_DATA_KEY_BYTES} bytes."
            ))
        });
    }

    let mut key_bytes = [0_u8; LOCAL_DATA_KEY_BYTES];
    SystemRandom::new()
        .fill(&mut key_bytes)
        .map_err(|_| AppError::Crypto("Failed to generate a local data key.".to_string()))?;
    if store_generic_password(&key_bytes)? {
        Ok(key_bytes)
    } else if let Some(existing_key_bytes) = load_generic_password()? {
        existing_key_bytes.try_into().map_err(|_| {
            AppError::Crypto(format!(
                "Stored Keychain master key must be exactly {LOCAL_DATA_KEY_BYTES} bytes."
            ))
        })
    } else {
        Err(AppError::Crypto(
            "The macOS Keychain master key could not be recovered after a duplicate item response."
                .to_string(),
        ))
    }
}

fn load_generic_password() -> AppResult<Option<Vec<u8>>> {
    let mut password_length = 0_u32;
    let mut password_data = ptr::null_mut();
    let mut item_ref = ptr::null_mut();
    let status = unsafe {
        SecKeychainFindGenericPassword(
            ptr::null(),
            APP_BUNDLE_ID.len() as u32,
            APP_BUNDLE_ID.as_ptr().cast(),
            LOCAL_DATA_KEY_ACCOUNT.len() as u32,
            LOCAL_DATA_KEY_ACCOUNT.as_ptr().cast(),
            &mut password_length,
            &mut password_data,
            &mut item_ref,
        )
    };

    match status {
        0 => {
            let bytes = unsafe {
                std::slice::from_raw_parts(password_data.cast::<u8>(), password_length as usize)
                    .to_vec()
            };
            let free_status = unsafe { SecKeychainItemFreeContent(ptr::null_mut(), password_data) };
            if free_status != 0 {
                return Err(AppError::Crypto(format!(
                    "Failed to release Keychain password content (OSStatus {free_status})."
                )));
            }
            Ok(Some(bytes))
        }
        ERR_SEC_ITEM_NOT_FOUND => Ok(None),
        _ => Err(AppError::Crypto(format!(
            "Failed to read the macOS Keychain master key (OSStatus {status})."
        ))),
    }
}

fn store_generic_password(key_bytes: &[u8; LOCAL_DATA_KEY_BYTES]) -> AppResult<bool> {
    let status = unsafe {
        SecKeychainAddGenericPassword(
            ptr::null(),
            APP_BUNDLE_ID.len() as u32,
            APP_BUNDLE_ID.as_ptr().cast(),
            LOCAL_DATA_KEY_ACCOUNT.len() as u32,
            LOCAL_DATA_KEY_ACCOUNT.as_ptr().cast(),
            key_bytes.len() as u32,
            key_bytes.as_ptr().cast(),
            ptr::null_mut(),
        )
    };

    match status {
        0 => Ok(true),
        ERR_SEC_DUPLICATE_ITEM => Ok(false),
        _ => Err(AppError::Crypto(format!(
            "Failed to store the macOS Keychain master key (OSStatus {status})."
        ))),
    }
}
