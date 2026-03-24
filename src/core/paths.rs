use std::{fs, path::PathBuf};

#[cfg(not(target_os = "macos"))]
use crate::constants::crypto::LOCAL_DATA_KEY_FILE_NAME;
use crate::core::error::AppResult;

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub root: PathBuf,
    pub config_file: PathBuf,
    pub history_db: PathBuf,
    pub cert_der: PathBuf,
    pub key_der: PathBuf,
    pub inbox_dir: PathBuf,
    #[cfg(not(target_os = "macos"))]
    pub local_data_key_file: PathBuf,
}

impl AppPaths {
    pub fn from_root(root: PathBuf) -> Self {
        Self {
            config_file: root.join("config.json"),
            history_db: root.join("history.sqlite3"),
            cert_der: root.join("device.cert.der"),
            key_der: root.join("device.key.der"),
            inbox_dir: root.join("incoming"),
            #[cfg(not(target_os = "macos"))]
            local_data_key_file: root.join(LOCAL_DATA_KEY_FILE_NAME),
            root,
        }
    }

    pub fn ensure(&self) -> AppResult<()> {
        for dir in [&self.root, &self.inbox_dir] {
            fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}
