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

    #[cfg(test)]
    fn with_path(settings_path: PathBuf) -> Self {
        Self { settings_path }
    }

    pub fn load_or_default(&self) -> Result<PersistedSettings, AppError> {
        self.restore_backup_if_primary_missing();

        if !self.settings_path.exists() {
            return Ok(PersistedSettings::default());
        }

        let raw = std::fs::read_to_string(&self.settings_path)?;
        let settings =
            serde_json::from_str::<PersistedSettings>(crate::util::strip_utf8_bom(&raw))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn unique_settings_path() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "yuuko_settings_test_{}_{}.json",
            std::process::id(),
            n
        ))
    }

    #[test]
    fn load_or_default_tolerates_utf8_bom() {
        // 手編集で付くBOM付き設定JSONも読めること（起動ブロックを防ぐ）。
        let path = unique_settings_path();
        let json = serde_json::to_string_pretty(&PersistedSettings::default()).unwrap();
        std::fs::write(&path, format!("\u{feff}{json}")).unwrap();
        let repo = SettingsRepository::with_path(path.clone());
        let loaded = repo
            .load_or_default()
            .expect("BOM-prefixed settings should load");
        let _ = std::fs::remove_file(&path);
        assert_eq!(loaded.version, PersistedSettings::default().version);
    }

    #[test]
    fn load_or_default_still_errors_on_corrupt_json() {
        // BOM以外の破損は現状どおりエラー（黙ってデフォルトに戻さない）。
        let path = unique_settings_path();
        std::fs::write(&path, b"{ not valid json").unwrap();
        let repo = SettingsRepository::with_path(path.clone());
        let result = repo.load_or_default();
        let _ = std::fs::remove_file(&path);
        assert!(result.is_err());
    }
}
