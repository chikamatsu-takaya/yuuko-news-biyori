use std::path::PathBuf;

use crate::domain::settings::PersistedSettings;
use crate::error::AppError;
use crate::paths::AppPaths;

#[derive(Debug, Clone)]
pub struct SettingsRepository {
    settings_path: PathBuf,
}

impl SettingsRepository {
    pub fn new(paths: &AppPaths) -> Self {
        Self {
            settings_path: paths.settings_path.clone(),
        }
    }

    pub fn load_or_default(&self) -> Result<PersistedSettings, AppError> {
        self.restore_backup_if_primary_missing();

        if !self.settings_path.exists() {
            return Ok(PersistedSettings::default());
        }

        let raw = std::fs::read_to_string(&self.settings_path)?;
        let settings = serde_json::from_str::<PersistedSettings>(&raw)?;
        Ok(settings)
    }

    pub fn exists(&self) -> bool {
        self.settings_path.exists()
    }

    pub fn save(&self, settings: &PersistedSettings) -> Result<(), AppError> {
        if let Some(parent) = self.settings_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let temp_path = self.settings_path.with_extension("json.tmp");
        let backup_path = self.settings_path.with_extension("json.bak");
        let payload = serde_json::to_vec_pretty(settings)?;
        std::fs::write(&temp_path, payload)?;

        let had_existing = self.settings_path.exists();
        if had_existing {
            if backup_path.exists() {
                std::fs::remove_file(&backup_path)?;
            }
            std::fs::rename(&self.settings_path, &backup_path)?;
        }

        match std::fs::rename(&temp_path, &self.settings_path) {
            Ok(()) => {
                if had_existing && backup_path.exists() {
                    if let Err(error) = std::fs::remove_file(&backup_path) {
                        log::warn!("Failed to remove settings backup: {error}");
                    }
                }
                Ok(())
            }
            Err(error) => {
                log::error!("Failed to promote temporary settings file: {error}");

                if had_existing && backup_path.exists() {
                    if let Err(restore_error) = std::fs::rename(&backup_path, &self.settings_path) {
                        log::error!("Failed to restore settings backup: {restore_error}");
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
        if self.settings_path.exists() {
            return;
        }

        let backup_path = self.settings_path.with_extension("json.bak");
        if !backup_path.exists() {
            return;
        }

        log::warn!("settings.json is missing. attempting backup restore.");
        if let Err(error) = std::fs::rename(&backup_path, &self.settings_path) {
            log::error!("Failed to restore settings backup: {error}");
        }
    }
}
