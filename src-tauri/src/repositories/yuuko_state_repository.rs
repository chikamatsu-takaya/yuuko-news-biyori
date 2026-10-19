use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::domain::yuuko::PersistedYuukoState;
use crate::error::AppError;
use crate::infra::atomic_file;
use crate::paths::AppPaths;

/// ゆうこの通知状態（`yuuko_notification_state.json`）の読み書き。
///
/// ゆうこ窓の操作（閉じる・無視・クリック）、メイン画面の5分ごとの判定、デスクトップ通知スレッドが同時に
/// 「読み→変更→保存」しうるため、呼び出し側（YuukoService）が `lock_updates` を取ってから行う
/// （日次回数・クールタイム・「閉じた」記録を後勝ちで消さないため）。
/// 保存は一時ファイル→1回の rename（`infra::atomic_file`）で、本体が無い瞬間・書きかけの中身は見えない。
#[derive(Debug, Clone)]
pub struct YuukoStateRepository {
    state_path: PathBuf,
    update_lock: Arc<Mutex<()>>,
}

impl YuukoStateRepository {
    pub fn new(paths: &AppPaths) -> Self {
        Self::with_path(paths.yuuko_state_path.clone())
    }

    pub(crate) fn with_path(state_path: PathBuf) -> Self {
        Self {
            update_lock: atomic_file::shared_path_lock(&state_path),
            state_path,
        }
    }

    /// 通知状態の「読み→変更→保存」を直列にするロック（パスごとにプロセス全体で共有）。
    /// 保持したまま `load_or_default` / `save` を呼ぶ（どちらも内部ではロックを取らない）。
    pub fn lock_updates(&self) -> MutexGuard<'_, ()> {
        atomic_file::lock_ignoring_poison(&self.update_lock, "yuuko-state")
    }

    pub fn load_or_default(&self) -> Result<PersistedYuukoState, AppError> {
        atomic_file::restore_legacy_backup_if_missing(&self.state_path, "yuuko-state");

        // 「無い」ときだけ既定値。存在確認自体に失敗したとき（権限など）はエラーを返す。
        if !self.state_path.try_exists()? {
            return Ok(PersistedYuukoState::default());
        }

        // 手編集で付くUTF-8 BOMは除去してから解析する（settings側と同方針）。
        // JSON として読めない場合は `yuuko_notification_state.corrupt.json` へ退避して既定値で作り直す
        // （ゆうこ登場・通知挙動_詳細設計書 §16.3: 不正状態は待機中へ戻す / セキュリティ詳細設計書 §11.4）。
        // 退避に失敗したら上書きせずエラーを返す。次回の読み込みで再試行されるため通知が止まり続けない。
        super::corrupt_json::read_json_or_reset(&self.state_path, "yuuko-state", || {
            let state = PersistedYuukoState::default();
            self.save(&state)?;
            Ok(state)
        })
    }

    pub fn exists(&self) -> bool {
        self.state_path.exists()
    }

    pub fn save(&self, state: &PersistedYuukoState) -> Result<(), AppError> {
        atomic_file::write_json_atomic(&self.state_path, state, "yuuko-state")
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
        (YuukoStateRepository::with_path(path.clone()), path)
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
    fn concurrent_locked_updates_do_not_lose_writes() {
        // ゆうこ窓・メイン画面・通知スレッドが別インスタンスから同時に更新しても、日次回数が失われない。
        let (repo, path) = temp_repo();
        repo.save(&PersistedYuukoState::default()).unwrap();

        let threads = 4u32;
        let rounds = 25u32;
        let handles = (0..threads)
            .map(|_| {
                let repo = YuukoStateRepository::with_path(path.clone());
                std::thread::spawn(move || {
                    for _ in 0..rounds {
                        let _guard = repo.lock_updates();
                        let mut state = repo.load_or_default().unwrap();
                        state.daily_notification.count += 1;
                        repo.save(&state).unwrap();
                    }
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            handle.join().unwrap();
        }

        let loaded = repo.load_or_default().unwrap();
        assert_eq!(loaded.daily_notification.count, threads * rounds);
        // 同時に書いても一時ファイル名が衝突せず、書きかけが本体へ昇格しない（破損退避も起きない）。
        assert!(!path.with_extension("corrupt.json").exists());
        cleanup(&path);
    }

    fn cleanup_corrupt(path: &std::path::Path) {
        cleanup(path);
        let corrupt = path.with_extension("corrupt.json");
        if corrupt.is_dir() {
            let _ = std::fs::remove_dir_all(&corrupt);
        } else {
            let _ = std::fs::remove_file(&corrupt);
        }
        let _ = std::fs::remove_file(path.with_extension("corrupt.json.tmp"));
    }

    #[test]
    fn load_or_default_backs_up_corrupt_json_and_resets_to_defaults() {
        // 読めない通知状態は退避して待機中の既定値へ戻す（§16.3 / セキュリティ詳細設計書 §11.4）。
        let (repo, path) = temp_repo();
        std::fs::write(&path, b"{ not valid json").unwrap();

        let loaded = repo
            .load_or_default()
            .expect("corrupt state should be reset");

        assert_eq!(loaded.state, PersistedYuukoState::default().state);
        assert!(loaded.introduced_article_ids.is_empty());
        assert_eq!(
            std::fs::read(path.with_extension("corrupt.json")).unwrap(),
            b"{ not valid json"
        );
        // 作り直したファイルは次回そのまま読める。
        let reloaded = repo.load_or_default().expect("reset state should load");
        assert_eq!(reloaded.state, loaded.state);
        assert!(!path.with_extension("corrupt.json.tmp").exists());

        cleanup_corrupt(&path);
    }

    #[test]
    fn load_or_default_backs_up_invalid_utf8_as_corrupt() {
        // 不正な UTF-8（バイナリ化など）も「JSON として読めない」側で扱い、退避して作り直す。
        let (repo, path) = temp_repo();
        std::fs::write(&path, [0xFFu8, 0xFE, 0x00, 0x7B]).unwrap();

        repo.load_or_default()
            .expect("invalid utf-8 should be reset");

        assert_eq!(
            std::fs::read(path.with_extension("corrupt.json")).unwrap(),
            vec![0xFFu8, 0xFE, 0x00, 0x7B]
        );

        cleanup_corrupt(&path);
    }

    #[test]
    fn load_or_default_keeps_corrupt_file_when_backup_fails() {
        // 退避に失敗したら上書きせず、エラーを返して元ファイルを残す（D57）。
        let (repo, path) = temp_repo();
        std::fs::write(&path, b"{ not valid json").unwrap();
        std::fs::create_dir_all(path.with_extension("corrupt.json")).unwrap();

        let result = repo.load_or_default();

        assert!(matches!(result, Err(AppError::Io(_))));
        assert_eq!(std::fs::read(&path).unwrap(), b"{ not valid json");
        assert!(!path.with_extension("corrupt.json.tmp").exists());

        // 退避できるようになれば次回の読み込みで回復する（通知が止まり続けない）。
        let _ = std::fs::remove_dir_all(path.with_extension("corrupt.json"));
        assert!(repo.load_or_default().is_ok());

        cleanup_corrupt(&path);
    }

    #[test]
    fn load_or_default_does_not_reset_on_io_error() {
        // 読み込み自体の IO エラー（ここではパスがディレクトリ）は作り直さずにエラーを返す。
        let (repo, path) = temp_repo();
        std::fs::create_dir_all(&path).unwrap();

        let result = repo.load_or_default();

        assert!(matches!(result, Err(AppError::Io(_))));
        assert!(path.is_dir());
        assert!(!path.with_extension("corrupt.json").exists());

        let _ = std::fs::remove_dir_all(&path);
    }
}
