use std::path::{Path, PathBuf};

use crate::error::AppError;

pub const SETTINGS_RELATIVE_PATH: &str = "config/settings.json";

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub app_data_dir: PathBuf,
    pub settings_path: PathBuf,
}

impl AppPaths {
    pub fn new(app_data_dir: PathBuf) -> Self {
        let settings_path = app_data_dir.join(SETTINGS_RELATIVE_PATH);
        Self {
            app_data_dir,
            settings_path,
        }
    }

    pub fn ensure_storage_dirs(&self) -> Result<(), AppError> {
        ensure_parent_dir(&self.settings_path)
    }
}

fn ensure_parent_dir(path: &Path) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}
