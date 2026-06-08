//! 友情ランク状態の保存・読込（`user/friendship.json`）。
//!
//! yuuko_state_repository と同じ atomic 方式（tmp に書く → 既存を bak へ退避 → tmp を昇格、
//! 失敗時は bak から復旧）で書き込み、保存中の破損で状態を失わないようにする。

use std::path::PathBuf;

use crate::domain::friendship::FriendshipState;
use crate::error::AppError;
use crate::paths::AppPaths;

#[derive(Debug, Clone)]
pub struct FriendshipRepository {
    state_path: PathBuf,
}

impl FriendshipRepository {
    pub fn new(paths: &AppPaths) -> Self {
        Self {
            state_path: paths.friendship_path.clone(),
        }
    }

    pub fn load_or_default(&self) -> Result<FriendshipState, AppError> {
        self.restore_backup_if_primary_missing();

        if !self.state_path.exists() {
            return Ok(FriendshipState::default());
        }

        let raw = std::fs::read_to_string(&self.state_path)?;
        let state = serde_json::from_str::<FriendshipState>(&raw)?;
        Ok(state)
    }

    pub fn exists(&self) -> bool {
        self.state_path.exists()
    }

    pub fn save(&self, state: &FriendshipState) -> Result<(), AppError> {
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
                        log::warn!("Failed to remove friendship-state backup: {error}");
                    }
                }
                Ok(())
            }
            Err(error) => {
                log::error!("Failed to promote temporary friendship-state file: {error}");

                if had_existing && backup_path.exists() {
                    if let Err(restore_error) = std::fs::rename(&backup_path, &self.state_path) {
                        log::error!("Failed to restore friendship-state backup: {restore_error}");
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

        log::warn!("friendship.json is missing. attempting backup restore.");
        if let Err(error) = std::fs::rename(&backup_path, &self.state_path) {
            log::error!("Failed to restore friendship-state backup: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::friendship::FriendshipEventType;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_repo() -> (FriendshipRepository, PathBuf) {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "yuuko-friendship-tests-{}-{unique}",
            std::process::id()
        ));
        let state_path = root.join("user").join("friendship.json");
        (FriendshipRepository { state_path }, root)
    }

    #[test]
    fn round_trip_persists_earned_points() {
        let (repo, root) = temp_repo();
        assert!(!repo.exists());

        let mut state = repo.load_or_default().unwrap();
        state.earn(
            FriendshipEventType::YuukoToMain,
            "2026-06-08",
            "2026-06-08T00:00:00Z",
        ); // +5
        repo.save(&state).unwrap();
        assert!(repo.exists());

        let reloaded = repo.load_or_default().unwrap();
        assert_eq!(reloaded.total_points, 5);
        assert_eq!(reloaded.daily_points.points, 5);
        assert_eq!(reloaded.daily_points.date, "2026-06-08");

        let _ = std::fs::remove_dir_all(&root);
    }
}
