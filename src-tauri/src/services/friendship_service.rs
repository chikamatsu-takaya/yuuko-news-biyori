//! 友情ランクサービス。状態の読み取りと、イベントによるポイント加算（ランクアップ判定込み）を担う。
//!
//! サーバ(Rust)を正とし、**デイリー上限・有効イベント検証・ランクアップ判定をここで強制**する。
//! フロントはイベント発生を `record_friendship_event` で伝えるだけで、加算量や上限は決められない。

use crate::domain::friendship::{
    FriendshipEventType, FriendshipStateDto, RecordFriendshipEventResult,
};
use crate::error::AppError;
use crate::repositories::friendship_repository::FriendshipRepository;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct FriendshipService {
    friendship_repository: FriendshipRepository,
    store_lock: Arc<Mutex<()>>,
}

impl FriendshipService {
    pub fn new(friendship_repository: FriendshipRepository) -> Self {
        Self {
            friendship_repository,
            store_lock: Arc::new(Mutex::new(())),
        }
    }

    /// 初回起動時に既定状態を保存する（未保存なら）。
    pub fn initialize_default_if_missing(&self) -> Result<(), AppError> {
        let _guard = self.lock_store()?;
        if !self.friendship_repository.exists() {
            let state = self.friendship_repository.load_or_default()?;
            self.friendship_repository.save(&state)?;
        }
        Ok(())
    }

    /// 現在の友情ランク状態（読み取り）。
    pub fn get_friendship_state(&self) -> Result<FriendshipStateDto, AppError> {
        let _guard = self.lock_store()?;
        Ok(self.friendship_repository.load_or_default()?.to_dto())
    }

    /// イベントによるポイント加算。無効なイベント種別は Validation エラーとする。
    /// デイリー上限・ランクアップ判定はドメイン側で行い、結果を保存して返す。
    pub fn record_friendship_event(
        &self,
        event_type: &str,
    ) -> Result<RecordFriendshipEventResult, AppError> {
        let event = FriendshipEventType::from_storage(event_type).ok_or_else(|| {
            AppError::Validation(format!("unknown friendship event type: {event_type}"))
        })?;

        let _guard = self.lock_store()?;
        let mut state = self.friendship_repository.load_or_default()?;
        let outcome = state.earn(event, &current_utc_date(), &current_utc_timestamp());
        self.friendship_repository.save(&state)?;

        Ok(RecordFriendshipEventResult {
            ranked_up: outcome.ranked_up,
            new_rank: state.current_rank,
            earned_point: outcome.earned_points,
            state: state.to_dto(),
        })
    }

    fn lock_store(&self) -> Result<std::sync::MutexGuard<'_, ()>, AppError> {
        self.store_lock
            .lock()
            .map_err(|_| AppError::Io(std::io::Error::other("friendship store lock was poisoned")))
    }
}

/// 当日の日付（UTC・"YYYY-MM-DD"）。デイリーリセット判定に使う。
fn current_utc_date() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

/// 現在時刻（UTC・"YYYY-MM-DDThh:mm:ssZ"）。ランクアップ時刻記録に使う。
fn current_utc_timestamp() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::AppPaths;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_service() -> (FriendshipService, std::path::PathBuf) {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "yuuko-friendship-service-tests-{}-{unique}",
            std::process::id()
        ));
        let paths = AppPaths::new(root.clone());
        let service = FriendshipService::new(FriendshipRepository::new(&paths));
        (service, root)
    }

    #[test]
    fn cloned_services_serialize_friendship_event_updates() {
        let (service, root) = temp_service();
        let handles = (0..20)
            .map(|_| {
                let service = service.clone();
                std::thread::spawn(move || {
                    service.record_friendship_event("term_explained").unwrap();
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle.join().unwrap();
        }

        let state = service.get_friendship_state().unwrap();
        assert_eq!(state.current_point, 20);
        assert_eq!(state.daily_earned_point, 20);

        let _ = std::fs::remove_dir_all(root);
    }
}
