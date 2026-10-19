//! 報酬状態の保存・読込（`rewards/rewards.json`・データ設計書 §11）。
//!
//! friendship_repository と同じ atomic 方式（tmp に書く → 既存を bak へ退避 → tmp を昇格、
//! 失敗時は bak から復旧）で書き込み、保存中の破損で解放済み報酬を失わないようにする。

use std::path::PathBuf;

use crate::domain::reward::RewardsState;
use crate::error::AppError;
use crate::paths::AppPaths;

#[derive(Debug, Clone)]
pub struct RewardRepository {
    state_path: PathBuf,
}

impl RewardRepository {
    pub fn new(paths: &AppPaths) -> Self {
        Self {
            state_path: paths.rewards_path.clone(),
        }
    }

    /// 保存済みの報酬状態を読む。未保存なら `None`（呼び出し側が旧データから作る）。
    /// JSON として読めない場合は `rewards.corrupt.json` へ退避して既定値（空）で作り直す
    /// （セキュリティ詳細設計書 §11.4）。解放済み報酬は友情ランクから冪等に導出し直されるため
    /// （`RewardsState::unlock_up_to_rank`）失われないが、確認済みだった報酬は未確認として再び現れる。
    /// 退避に失敗したら上書きせずエラーを返す。
    pub fn load(&self) -> Result<Option<RewardsState>, AppError> {
        self.restore_backup_if_primary_missing();

        if !self.state_path.exists() {
            return Ok(None);
        }

        let state = super::corrupt_json::read_json_or_reset(&self.state_path, "rewards", || {
            let state = RewardsState::default();
            self.save(&state)?;
            Ok(state)
        })?;
        Ok(Some(state))
    }

    /// rewards.json（または復旧可能な bak）があるか。無ければ旧データからの移行が必要。
    pub fn exists(&self) -> bool {
        self.state_path.exists() || self.state_path.with_extension("json.bak").exists()
    }

    pub fn save(&self, state: &RewardsState) -> Result<(), AppError> {
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
                        log::warn!("Failed to remove rewards-state backup: {error}");
                    }
                }
                Ok(())
            }
            Err(error) => {
                log::error!("Failed to promote temporary rewards-state file: {error}");

                if had_existing && backup_path.exists() {
                    if let Err(restore_error) = std::fs::rename(&backup_path, &self.state_path) {
                        log::error!("Failed to restore rewards-state backup: {restore_error}");
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

        log::warn!("rewards.json is missing. attempting backup restore.");
        if let Err(error) = std::fs::rename(&backup_path, &self.state_path) {
            log::error!("Failed to restore rewards-state backup: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_repo() -> (RewardRepository, PathBuf) {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "yuuko-reward-repo-tests-{}-{unique}",
            std::process::id()
        ));
        let paths = AppPaths::new(root.clone());
        (RewardRepository::new(&paths), root)
    }

    #[test]
    fn round_trip_persists_unlocked_and_pending() {
        let (repo, root) = temp_repo();
        assert!(repo.load().unwrap().is_none());

        let mut state = RewardsState::default();
        state.unlock_up_to_rank(7, "2026-10-07T00:00:00Z");
        state.confirm(&["theme_001".to_string()]);
        repo.save(&state).unwrap();
        // 再保存（bak 経由の置き換え）でも壊れない。
        repo.save(&state).unwrap();

        let reloaded = repo.load().unwrap().unwrap();
        assert_eq!(reloaded, state);
        let raw = std::fs::read_to_string(root.join("rewards").join("rewards.json")).unwrap();
        // §11.2 のフィールド名で保存する。
        assert!(raw.contains("\"unlockedRewardIds\""));
        assert!(raw.contains("\"pendingRewards\""));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn corrupt_file_is_backed_up_and_reset_to_defaults() {
        let (repo, root) = temp_repo();
        std::fs::create_dir_all(repo.state_path.parent().unwrap()).unwrap();
        std::fs::write(&repo.state_path, "{not json").unwrap();

        // 読めない rewards.json は退避してから空の既定値で作り直す（§11.4）。
        let loaded = repo.load().unwrap().unwrap();
        assert_eq!(loaded, RewardsState::default());
        let backup = root.join("rewards").join("rewards.corrupt.json");
        assert_eq!(std::fs::read(&backup).unwrap(), b"{not json");
        // 作り直したファイルは次回そのまま読める。
        assert_eq!(repo.load().unwrap().unwrap(), RewardsState::default());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn corrupt_file_is_kept_when_backup_fails() {
        let (repo, root) = temp_repo();
        std::fs::create_dir_all(repo.state_path.parent().unwrap()).unwrap();
        std::fs::write(&repo.state_path, "{not json").unwrap();
        // 退避先にディレクトリを置いて退避を失敗させる。
        std::fs::create_dir_all(root.join("rewards").join("rewards.corrupt.json")).unwrap();

        assert!(repo.load().is_err());
        // 上書きせず元のまま残す。
        assert_eq!(std::fs::read(&repo.state_path).unwrap(), b"{not json");

        let _ = std::fs::remove_dir_all(&root);
    }
}
