use std::{fs, path::PathBuf};

use crate::{
    constants::storage::{
        CONFIG_FILE_NAME, DEVICE_CERT_FILE_NAME, DEVICE_KEY_FILE_NAME, HISTORY_DB_FILE_NAME,
        INBOX_DIR_NAME,
    },
    core::error::AppResult,
};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub root: PathBuf,
    pub config_file: PathBuf,
    pub history_db: PathBuf,
    pub cert_der: PathBuf,
    pub key_der: PathBuf,
    pub inbox_dir: PathBuf,
}

impl AppPaths {
    pub fn from_root(root: PathBuf) -> Self {
        Self {
            config_file: root.join(CONFIG_FILE_NAME),
            history_db: root.join(HISTORY_DB_FILE_NAME),
            cert_der: root.join(DEVICE_CERT_FILE_NAME),
            key_der: root.join(DEVICE_KEY_FILE_NAME),
            inbox_dir: root.join(INBOX_DIR_NAME),
            root,
        }
    }

    pub fn ensure(&self) -> AppResult<()> {
        for dir in [&self.root, &self.inbox_dir] {
            fs::create_dir_all(dir)?;
        }
        Ok(())
    }

    pub fn remote_item_dir(&self, item_id: Uuid) -> PathBuf {
        self.inbox_dir.join(item_id.to_string())
    }
}
