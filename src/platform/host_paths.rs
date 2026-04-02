use directories::ProjectDirs;
use std::{env, path::PathBuf};

use crate::{
    constants::app::{
        ORGANIZATION_NAME, ORGANIZATION_QUALIFIER, PRODUCT_DIR_NAME, STORAGE_DIR_NAME,
    },
    core::{error::AppResult, paths::AppPaths},
};

/// Resolves the application storage root shared by desktop shells and fallback backends.
pub(super) fn discover_app_paths() -> AppResult<AppPaths> {
    let root = ProjectDirs::from(ORGANIZATION_QUALIFIER, ORGANIZATION_NAME, PRODUCT_DIR_NAME)
        .map(|dirs| dirs.data_local_dir().to_path_buf())
        .unwrap_or_else(fallback_storage_root);

    let paths = AppPaths::from_root(root);
    paths.ensure()?;
    Ok(paths)
}

fn fallback_storage_root() -> PathBuf {
    env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(format!(".{STORAGE_DIR_NAME}-data"))
}
