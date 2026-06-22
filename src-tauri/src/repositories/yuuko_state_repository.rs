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
        // 手編集で付くUTF-8 BOMを除去してからparseする（settings側と同方針）。
        let state = serde_json::from_str::<PersistedYuukoState>(crate::util::strip_utf8_bom(&raw))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::yuuko::YuukoResidentState;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_repo() -> (YuukoStateRepository, PathBuf) {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "yuuko_state_repo_{}_{}.json",
            std::process::id(),
            n
        ));
        (
            YuukoStateRepository {
                state_path: path.clone(),
            },
            path,
        )
    }

    fn cleanup(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("json.bak"));
        let _ = std::fs::remove_file(path.with_extension("json.tmp"));
    }

    #[test]
    fn load_or_default_tolerates_utf8_bom() {
        // 手編集で付くBOM付き state JSON も読めること（通知が出ない不具合の再発防止・settings と同方針）。
        let (repo, path) = temp_repo();
        let json = r#"{
  "state": "Waiting",
  "currentArticleId": null,
  "previewArticle": null,
  "rewardNotification": null,
  "lastNotifiedAt": null,
  "cooldownUntil": null,
  "dailyNotification": { "date": "2026-06-22", "count": 0 },
  "introducedArticleIds": []
}"#;
        std::fs::write(&path, format!("\u{feff}{json}")).unwrap();

        let loaded = repo
            .load_or_default()
            .expect("BOM-prefixed yuuko state should load");

        assert_eq!(loaded.state, YuukoResidentState::Waiting);
        assert_eq!(loaded.daily_notification.date, "2026-06-22");
        assert_eq!(loaded.daily_notification.count, 0);
        assert!(loaded.introduced_article_ids.is_empty());
        assert!(loaded.last_notified_at.is_none());
        assert!(loaded.cooldown_until.is_none());
        assert!(loaded.current_article_id.is_none());

        cleanup(&path);
    }

    #[test]
    fn load_or_default_reads_plain_json_without_bom() {
        // BOM なしの通常 JSON も従来どおり読めること（回帰防止）。
        let (repo, path) = temp_repo();
        let state = PersistedYuukoState {
            introduced_article_ids: vec!["article-001".to_string()],
            ..PersistedYuukoState::default()
        };
        std::fs::write(&path, serde_json::to_string_pretty(&state).unwrap()).unwrap();

        let loaded = repo.load_or_default().expect("plain json should load");
        assert_eq!(
            loaded.introduced_article_ids,
            vec!["article-001".to_string()]
        );

        cleanup(&path);
    }

    #[test]
    fn load_or_default_still_errors_on_corrupt_json() {
        // BOM以外の破損は従来どおりエラー（黙ってデフォルトに戻さない）。settings と同方針。
        let (repo, path) = temp_repo();
        std::fs::write(&path, b"{ not valid json").unwrap();

        let result = repo.load_or_default();

        cleanup(&path);
        assert!(result.is_err());
    }
}
