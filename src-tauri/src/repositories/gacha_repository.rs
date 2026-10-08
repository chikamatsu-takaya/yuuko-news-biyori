//! ガチャ状態の保存・読込（`gacha/gacha_state.json`・データ設計書 §12）。
//!
//! reward_repository と同じ atomic 方式（tmp に書く → 既存を bak へ退避 → tmp を昇格、
//! 失敗時は bak から復旧）で書き込み、保存中の破損で獲得済みのものやかけらを失わないようにする。
//! 獲得済みのものは他のデータから導出し直せないため（§12.2）、書き込み途中の破損は特に避ける。

use std::path::PathBuf;

use crate::domain::gacha::GachaState;
use crate::error::AppError;
use crate::paths::AppPaths;

#[derive(Debug, Clone)]
pub struct GachaRepository {
    state_path: PathBuf,
}

impl GachaRepository {
    pub fn new(paths: &AppPaths) -> Self {
        Self {
            state_path: paths.gacha_state_path.clone(),
        }
    }

    /// 保存済みのガチャ状態を読む。未保存なら `None`（呼び出し側が初期値を使う）。
    /// JSON として読めない場合は `gacha_state.corrupt.json` へ退避して既定値（初期かけら・所持なし）で
    /// 作り直す（§12.2・セキュリティ詳細設計書 §11.4）。獲得済みのものは退避ファイルにだけ残る。
    /// 退避に失敗したら上書きせずエラーを返す。
    pub fn load(&self) -> Result<Option<GachaState>, AppError> {
        self.restore_backup_if_primary_missing();

        if !self.state_path.exists() {
            return Ok(None);
        }

        let state = super::corrupt_json::read_json_or_reset(&self.state_path, "gacha", || {
            let state = GachaState::default();
            self.save(&state)?;
            Ok(state)
        })?;
        Ok(Some(state))
    }

    pub fn save(&self, state: &GachaState) -> Result<(), AppError> {
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
                        log::warn!("Failed to remove gacha-state backup: {}", error.kind());
                    }
                }
                Ok(())
            }
            Err(error) => {
                log::error!(
                    "Failed to promote temporary gacha-state file: {}",
                    error.kind()
                );

                if had_existing && backup_path.exists() {
                    if let Err(restore_error) = std::fs::rename(&backup_path, &self.state_path) {
                        log::error!(
                            "Failed to restore gacha-state backup: {}",
                            restore_error.kind()
                        );
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

        log::warn!("gacha_state.json is missing. attempting backup restore.");
        if let Err(error) = std::fs::rename(&backup_path, &self.state_path) {
            log::error!("Failed to restore gacha-state backup: {}", error.kind());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_repo() -> (GachaRepository, PathBuf) {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "yuuko-gacha-repo-tests-{}-{unique}",
            std::process::id()
        ));
        let paths = AppPaths::new(root.clone());
        (GachaRepository::new(&paths), root)
    }

    #[test]
    fn round_trip_persists_fragments_owned_and_new() {
        let (repo, root) = temp_repo();
        assert!(repo.load().unwrap().is_none());

        let state = GachaState {
            star_fragments: 43,
            owned_item_ids: vec!["card_001".to_string(), "gacha_theme_001".to_string()],
            new_item_ids: vec!["gacha_theme_001".to_string()],
            ..GachaState::default()
        };
        repo.save(&state).unwrap();
        // 再保存（bak 経由の置き換え）でも壊れない。
        repo.save(&state).unwrap();

        assert_eq!(repo.load().unwrap().unwrap(), state);
        let raw = std::fs::read_to_string(root.join("gacha").join("gacha_state.json")).unwrap();
        // §12.3 のフィールド名で保存する。
        assert!(raw.contains("\"starFragments\": 43"));
        assert!(raw.contains("\"ownedItemIds\""));
        assert!(raw.contains("\"newItemIds\""));
        assert!(!root.join("gacha").join("gacha_state.json.bak").exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn corrupt_file_is_backed_up_and_reset_to_initial_state() {
        let (repo, root) = temp_repo();
        std::fs::create_dir_all(repo.state_path.parent().unwrap()).unwrap();
        std::fs::write(&repo.state_path, "{not json").unwrap();

        // 読めない gacha_state.json は退避してから初期値で作り直す（§12.2）。
        let loaded = repo.load().unwrap().unwrap();
        assert_eq!(loaded, GachaState::default());
        let backup = root.join("gacha").join("gacha_state.corrupt.json");
        assert_eq!(std::fs::read(&backup).unwrap(), b"{not json");
        // 作り直したファイルは次回そのまま読める。
        assert_eq!(repo.load().unwrap().unwrap(), GachaState::default());

        // 再び壊れても退避ファイルは1世代だけ（上書き）。
        std::fs::write(&repo.state_path, "[broken again").unwrap();
        assert_eq!(repo.load().unwrap().unwrap(), GachaState::default());
        assert_eq!(std::fs::read(&backup).unwrap(), b"[broken again");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn corrupt_file_is_kept_when_backup_fails() {
        let (repo, root) = temp_repo();
        std::fs::create_dir_all(repo.state_path.parent().unwrap()).unwrap();
        std::fs::write(&repo.state_path, "{not json").unwrap();
        // 退避先にディレクトリを置いて退避を失敗させる。
        std::fs::create_dir_all(root.join("gacha").join("gacha_state.corrupt.json")).unwrap();

        assert!(repo.load().is_err());
        // 上書きせず元のまま残す。
        assert_eq!(std::fs::read(&repo.state_path).unwrap(), b"{not json");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_primary_is_restored_from_backup() {
        let (repo, root) = temp_repo();
        let state = GachaState {
            star_fragments: 7,
            ..GachaState::default()
        };
        std::fs::create_dir_all(repo.state_path.parent().unwrap()).unwrap();
        std::fs::write(
            repo.state_path.with_extension("json.bak"),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();

        assert_eq!(repo.load().unwrap().unwrap(), state);

        let _ = std::fs::remove_dir_all(&root);
    }
}
