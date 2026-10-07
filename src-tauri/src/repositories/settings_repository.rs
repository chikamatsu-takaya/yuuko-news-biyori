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
    pub(crate) fn with_path(settings_path: PathBuf) -> Self {
        Self { settings_path }
    }

    pub fn load_or_default(&self) -> Result<PersistedSettings, AppError> {
        self.restore_backup_if_primary_missing();

        if !self.settings_path.exists() {
            return Ok(PersistedSettings::default());
        }

        let raw = std::fs::read_to_string(&self.settings_path)?;
        let mut settings =
            serde_json::from_str::<PersistedSettings>(crate::util::strip_utf8_bom(&raw))?;
        settings.normalize_after_load();
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

    /// 破損した設定ファイルの退避先（`settings.json` → 同じフォルダの `settings.corrupt.json`）。
    /// 名前を固定にして常に1世代だけ残す（判断台帳 D57 / セキュリティ詳細設計書 §11.4）。
    fn corrupt_backup_path(&self) -> PathBuf {
        self.settings_path.with_extension("corrupt.json")
    }

    /// 破損した設定ファイルを、初期化で上書きする前に別名でそのまま複製する。
    /// 元ファイルは動かさない（初期化が失敗しても元のまま残すため）。
    /// 一時ファイルへ複製してから差し替えるので、途中で失敗しても前回の退避ファイルは壊れない。
    /// 失敗時は Err を返し、呼び出し側は初期化を中止する。
    pub fn backup_corrupt_file(&self) -> Result<(), AppError> {
        let backup_path = self.corrupt_backup_path();
        let temp_path = self.settings_path.with_extension("corrupt.json.tmp");

        let result = std::fs::copy(&self.settings_path, &temp_path)
            .and_then(|_| std::fs::rename(&temp_path, &backup_path));
        if let Err(error) = result {
            // 中身やフルパスはログへ出さない。
            log::error!("Failed to back up corrupt settings file: {}", error.kind());
            if temp_path.exists() {
                let _ = std::fs::remove_file(&temp_path);
            }
            return Err(error.into());
        }
        Ok(())
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

    #[test]
    fn load_or_default_normalizes_empty_work_time_ranges() {
        let path = unique_settings_path();
        let json = r#"{
            "version": 1,
            "notification": {
                "enabled": true,
                "mode": "random_in_work_time",
                "workTimeRanges": [],
                "maxPerDay": 3
            }
        }"#;
        std::fs::write(&path, json).unwrap();
        let repo = SettingsRepository::with_path(path.clone());
        let loaded = repo
            .load_or_default()
            .expect("settings with empty workTimeRanges should load");
        let _ = std::fs::remove_file(&path);

        assert_eq!(loaded.notification.work_time_ranges.len(), 2);
        assert_eq!(loaded.notification.work_time_ranges[0].start, "09:00");
        assert_eq!(loaded.notification.work_time_ranges[0].end, "12:00");
        assert_eq!(loaded.notification.work_time_ranges[1].start, "13:00");
        assert_eq!(loaded.notification.work_time_ranges[1].end, "18:00");
    }
}
