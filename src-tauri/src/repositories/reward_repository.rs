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

    /// 一時ファイル→1回の rename で置き換える（`infra::atomic_file`。本体が無くなる隙間を作らない）。
    pub fn save(&self, state: &RewardsState) -> Result<(), AppError> {
        crate::infra::atomic_file::write_json_atomic(&self.state_path, state, "rewards-state")
    }

    /// 旧形式の保存（本体→.bak 退避→昇格）が途中で止まって残った .bak を、本体が無いときだけ戻す。
    fn restore_backup_if_primary_missing(&self) {
        crate::infra::atomic_file::restore_legacy_backup_if_missing(
            &self.state_path,
            "rewards-state",
        );
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
