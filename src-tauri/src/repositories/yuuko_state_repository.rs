use std::path::PathBuf;

use crate::domain::yuuko::PersistedYuukoState;
use crate::error::AppError;
use crate::paths::AppPaths;

#[derive(Debug, Clone)]
pub struct YuukoStateRepository {
    state_path: PathBuf,
}

impl YuukoStateRepository {
    pub fn new(paths: &AppPaths) -> Self {
        Self {
            state_path: paths.yuuko_state_path.clone(),
        }
    }

    pub fn load_or_default(&self) -> Result<PersistedYuukoState, AppError> {
        self.restore_backup_if_primary_missing();

        if !self.state_path.exists() {
            return Ok(PersistedYuukoState::default());
        }

        let raw = std::fs::read_to_string(&self.state_path)?;
        let state = serde_json::from_str::<PersistedYuukoState>(&raw)?;
        Ok(state)
    }

    pub fn exists(&self) -> bool {
        self.state_path.exists()
    }

    pub fn save(&self, state: &PersistedYuukoState) -> Result<(), AppError> {
        if let Some(parent) = self.state_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let temp_path = self.state_path.with_extension("json.tmp");
        let backup_path = self.state_path.with_extension("json.bak");
        let payload = serde_json::to_vec_pretty(state)?;
        std::fs::write(&temp_path, payload)?;

        let had_existing = self.state_path.exists();
        if had_existing {
            if backup_path.exists() {
                std::fs::remove_file(&backup_path)?;
            }
            std::fs::rename(&self.state_path, &backup_path)?;
        }

        match std::fs::rename(&temp_path, &self.state_path) {
            Ok(()) => {
                if had_existing && backup_path.exists() {
                    if let Err(error) = std::fs::remove_file(&backup_path) {
                        log::warn!("Failed to remove yuuko-state backup: {error}");
                    }
                }
                Ok(())
            }
            Err(error) => {
                log::error!("Failed to promote temporary yuuko-state file: {error}");

                if had_existing && backup_path.exists() {
                    if let Err(restore_error) = std::fs::rename(&backup_path, &self.state_path) {
                        log::error!("Failed to restore yuuko-state backup: {restore_error}");
                    }
                }

                if temp_path.exists() {
                    let _ = std::fs::remove_file(&temp_path);
                }

                Err(error.into())
            }
        }
    }

    fn restore_backup_if_primary_missing(&self) {
        if self.state_path.exists() {
            return;
        }

        let backup_path = self.state_path.with_extension("json.bak");
        if !backup_path.exists() {
            return;
        }

        log::warn!("yuuko-notification-state.json is missing. attempting backup restore.");
        if let Err(error) = std::fs::rename(&backup_path, &self.state_path) {
            log::error!("Failed to restore yuuko-state backup: {error}");
        }
    }
}
