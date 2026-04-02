use std::io;

use thiserror::Error;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("SQLite error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[cfg_attr(target_os = "android", allow(dead_code))]
    #[error("Clipboard error: {0}")]
    Clipboard(String),
    #[error("Network error: {0}")]
    Network(String),
    #[error("Crypto error: {0}")]
    Crypto(String),
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

impl From<rcgen::Error> for AppError {
    fn from(value: rcgen::Error) -> Self {
        Self::Crypto(value.to_string())
    }
}
