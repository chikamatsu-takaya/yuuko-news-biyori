use std::path::{Path, PathBuf};

use crate::error::AppError;

pub const SETTINGS_RELATIVE_PATH: &str = "config/settings.json";
pub const YUUKO_STATE_RELATIVE_PATH: &str = "state/yuuko_notification_state.json";

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub app_data_dir: PathBuf,
    pub settings_path: PathBuf,
    pub yuuko_state_path: PathBuf,
}

impl AppPaths {
    pub fn new(app_data_dir: PathBuf) -> Self {
        let settings_path = app_data_dir.join(SETTINGS_RELATIVE_PATH);
        let yuuko_state_path = app_data_dir.join(YUUKO_STATE_RELATIVE_PATH);
        Self {
            app_data_dir,
            settings_path,
            yuuko_state_path,
        }
    }

    pub fn ensure_storage_dirs(&self) -> Result<(), AppError> {
        ensure_parent_dir(&self.settings_path)?;
        ensure_parent_dir(&self.yuuko_state_path)
    }
}

fn ensure_parent_dir(path: &Path) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}
