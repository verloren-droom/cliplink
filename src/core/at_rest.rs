use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::Path,
    sync::Arc,
};

use ring::{
    aead::{self, Aad, LessSafeKey, Nonce, UnboundKey},
    rand::{SecureRandom, SystemRandom},
};

use crate::{
    constants::crypto::{LOCAL_DATA_KEY_BYTES, LOCAL_DATA_MAGIC},
    core::error::{AppError, AppResult},
};

const CHACHA20_POLY1305_NONCE_BYTES: usize = 12;

/// Local authenticated-encryption helper used to protect static data stored on disk.
#[derive(Debug, Clone)]
pub struct LocalDataCipher {
    key_bytes: Arc<[u8; LOCAL_DATA_KEY_BYTES]>,
}

impl LocalDataCipher {
    pub fn from_bytes(bytes: [u8; LOCAL_DATA_KEY_BYTES]) -> Self {
        Self {
            key_bytes: Arc::new(bytes),
        }
    }

    pub fn from_slice(bytes: &[u8]) -> AppResult<Self> {
        let key_bytes: [u8; LOCAL_DATA_KEY_BYTES] = bytes.try_into().map_err(|_| {
            AppError::Crypto(format!(
                "Local data key must be exactly {LOCAL_DATA_KEY_BYTES} bytes."
            ))
        })?;
        Ok(Self::from_bytes(key_bytes))
    }

    pub fn seal(&self, purpose: &[u8], plaintext: &[u8]) -> AppResult<Vec<u8>> {
        let mut nonce_bytes = [0_u8; CHACHA20_POLY1305_NONCE_BYTES];
        SystemRandom::new()
            .fill(&mut nonce_bytes)
            .map_err(|_| AppError::Crypto("Failed to generate a local data nonce.".to_string()))?;

        let nonce = Nonce::assume_unique_for_key(nonce_bytes);
        let mut in_out = plaintext.to_vec();
        self.less_safe_key()?
            .seal_in_place_append_tag(nonce, Aad::from(purpose), &mut in_out)
            .map_err(|_| AppError::Crypto("Failed to encrypt local data.".to_string()))?;

        let mut sealed =
            Vec::with_capacity(LOCAL_DATA_MAGIC.len() + nonce_bytes.len() + in_out.len());
        sealed.extend_from_slice(LOCAL_DATA_MAGIC);
        sealed.extend_from_slice(&nonce_bytes);
        sealed.extend_from_slice(&in_out);
        Ok(sealed)
    }

    pub fn open(&self, purpose: &[u8], sealed: &[u8]) -> AppResult<Vec<u8>> {
        if !is_sealed_payload(sealed) {
            return Err(AppError::Crypto(
                "Local data payload is not sealed.".to_string(),
            ));
        }

        let nonce_offset = LOCAL_DATA_MAGIC.len();
        let ciphertext_offset = nonce_offset + CHACHA20_POLY1305_NONCE_BYTES;
        let nonce_bytes: [u8; CHACHA20_POLY1305_NONCE_BYTES] = sealed
            [nonce_offset..ciphertext_offset]
            .try_into()
            .map_err(|_| AppError::Crypto("Local data nonce is invalid.".to_string()))?;
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);
        let mut in_out = sealed[ciphertext_offset..].to_vec();
        let plaintext = self
            .less_safe_key()?
            .open_in_place(nonce, Aad::from(purpose), &mut in_out)
            .map_err(|_| AppError::Crypto("Failed to decrypt local data.".to_string()))?;
        Ok(plaintext.to_vec())
    }

    fn less_safe_key(&self) -> AppResult<LessSafeKey> {
        let unbound = UnboundKey::new(&aead::CHACHA20_POLY1305, self.key_bytes.as_ref())
            .map_err(|_| AppError::Crypto("Local data key is invalid.".to_string()))?;
        Ok(LessSafeKey::new(unbound))
    }
}

pub fn is_sealed_payload(bytes: &[u8]) -> bool {
    bytes.starts_with(LOCAL_DATA_MAGIC)
        && bytes.len() > LOCAL_DATA_MAGIC.len() + CHACHA20_POLY1305_NONCE_BYTES
}

pub fn load_sealed_bytes(
    path: &Path,
    purpose: &[u8],
    cipher: &LocalDataCipher,
) -> AppResult<Vec<u8>> {
    let bytes = fs::read(path)?;
    if !is_sealed_payload(&bytes) {
        return Err(AppError::Crypto(
            "Local data payload is not sealed.".to_string(),
        ));
    }
    cipher.open(purpose, &bytes)
}

pub fn save_sealed_bytes(
    path: &Path,
    purpose: &[u8],
    plaintext: &[u8],
    cipher: &LocalDataCipher,
) -> AppResult<()> {
    let sealed = cipher.seal(purpose, plaintext)?;
    replace_file_atomically(path, &sealed)?;
    Ok(())
}

pub(crate) fn replace_file_atomically(path: &Path, bytes: &[u8]) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let temp_path = path.with_extension("tmp");
    {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&temp_path, path)?;
    sync_parent_dir(path)?;
    Ok(())
}

pub(crate) fn sync_parent_dir(path: &Path) -> AppResult<()> {
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        File::open(parent)?.sync_all()?;
    }

    #[cfg(not(unix))]
    let _ = path;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::LocalDataCipher;

    #[test]
    fn local_data_roundtrip_works() {
        let cipher = LocalDataCipher::from_bytes([7_u8; 32]);
        let plaintext = b"secret clipboard payload";
        let sealed = cipher.seal(b"cliplink.test", plaintext).unwrap();
        let opened = cipher.open(b"cliplink.test", &sealed).unwrap();
        assert_eq!(opened, plaintext);
    }

    #[test]
    fn local_data_rejects_wrong_purpose() {
        let cipher = LocalDataCipher::from_bytes([9_u8; 32]);
        let sealed = cipher.seal(b"cliplink.test.one", b"payload").unwrap();
        assert!(cipher.open(b"cliplink.test.two", &sealed).is_err());
    }
}
