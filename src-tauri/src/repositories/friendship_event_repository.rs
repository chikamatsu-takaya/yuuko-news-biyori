//! 友情ポイント加算イベントの直近履歴（`user/friendship_events.json`・データ設計書 §10.3）。
//!
//! friendship.json と同じ atomic 方式（tmp に書く → 既存を bak へ退避 → tmp を昇格、失敗時は bak から復旧）で書く。
//! JSON として読めないときは `friendship_events.corrupt.json` へ1世代だけ退避して空の履歴から作り直す
//! （`corrupt_json::read_json_or_reset`）。履歴は補助的な記録なので、ポイント加算を止める理由にはしない
//! （呼び出し側はこのリポジトリのエラーをログだけにする）。

use std::path::PathBuf;

use crate::domain::friendship::{FriendshipEventLog, FriendshipEventRecord};
use crate::error::AppError;
use crate::paths::AppPaths;

#[derive(Debug, Clone)]
pub struct FriendshipEventRepository {
    path: PathBuf,
}

impl FriendshipEventRepository {
    pub fn new(paths: &AppPaths) -> Self {
        Self {
            path: paths.friendship_events_path.clone(),
        }
    }

    /// 履歴を読む。無ければ空、壊れていれば退避して空から作り直す（保存は次の追記で行う）。
    pub fn load(&self) -> Result<FriendshipEventLog, AppError> {
        self.restore_backup_if_primary_missing();
        if !self.path.exists() {
            return Ok(FriendshipEventLog::default());
        }
        super::corrupt_json::read_json_or_reset(&self.path, "friendship events", || {
            Ok(FriendshipEventLog::default())
        })
    }

    /// 1件追記し、直近 `FRIENDSHIP_EVENT_HISTORY_LIMIT` 件に切り詰めて保存する。
    /// 呼び出し側（友情サービス）の友情ロック下で呼ぶため、読み込み〜保存の間に他の追記は割り込まない。
    pub fn append(&self, record: FriendshipEventRecord) -> Result<(), AppError> {
        let mut log = self.load()?;
        log.push_truncated(record);
        self.save(&log)
    }

    /// 一時ファイル→1回の rename で置き換える（`infra::atomic_file`。本体が無くなる隙間を作らない）。
    fn save(&self, log: &FriendshipEventLog) -> Result<(), AppError> {
        crate::infra::atomic_file::write_json_atomic(&self.path, log, "friendship-events")
    }

    /// 旧形式の保存（本体→.bak 退避→昇格）が途中で止まって残った .bak を、本体が無いときだけ戻す。
    fn restore_backup_if_primary_missing(&self) {
        crate::infra::atomic_file::restore_legacy_backup_if_missing(
            &self.path,
            "friendship-events",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::friendship::FRIENDSHIP_EVENT_HISTORY_LIMIT;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_repo() -> (FriendshipEventRepository, PathBuf) {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "yuuko-friendship-events-tests-{}-{unique}",
            std::process::id()
        ));
        let repo = FriendshipEventRepository::new(&AppPaths::new(root.clone()));
        (repo, root)
    }

    fn record(created_at: &str) -> FriendshipEventRecord {
        FriendshipEventRecord {
            event_type: "news_detail_opened".to_string(),
            points: 3,
            created_at: created_at.to_string(),
        }
    }

    #[test]
    fn append_persists_and_truncates_to_most_recent() {
        let (repo, root) = temp_repo();
        for i in 0..(FRIENDSHIP_EVENT_HISTORY_LIMIT + 3) {
            repo.append(record(&format!("t{i:03}"))).unwrap();
        }
        let log = repo.load().unwrap();
        assert_eq!(log.events.len(), FRIENDSHIP_EVENT_HISTORY_LIMIT);
        assert_eq!(log.events[0].created_at, "t003");
        assert_eq!(
            log.events.last().unwrap().created_at,
            format!("t{:03}", FRIENDSHIP_EVENT_HISTORY_LIMIT + 2)
        );
        assert!(!root.join("user/friendship_events.json.tmp").exists());
        assert!(!root.join("user/friendship_events.json.bak").exists());

        // 保存形式（camelCase・外部由来の文字列を持たない）。
        let raw = std::fs::read_to_string(root.join("user/friendship_events.json")).unwrap();
        assert!(raw.contains("\"eventType\""));
        assert!(raw.contains("\"createdAt\""));
        assert!(!raw.contains("relatedArticleId"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn corrupt_file_is_backed_up_once_and_reset() {
        let (repo, root) = temp_repo();
        let path = root.join("user/friendship_events.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{ broken").unwrap();

        repo.append(record("t1")).unwrap();

        let log = repo.load().unwrap();
        assert_eq!(log.events, vec![record("t1")]);
        assert_eq!(
            std::fs::read(root.join("user/friendship_events.corrupt.json")).unwrap(),
            b"{ broken"
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
