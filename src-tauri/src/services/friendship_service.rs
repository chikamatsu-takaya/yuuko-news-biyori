//! 友情ランクサービス。状態の読み取りと、イベントによるポイント加算（ランクアップ判定込み）を担う。
//!
//! サーバ(Rust)を正とし、**デイリー上限・有効イベント検証・ランクアップ判定をここで強制**する。
//! フロントはイベント発生を `record_friendship_event` で伝えるだけで、加算量や上限は決められない。

use crate::domain::friendship::{
    FriendshipEventType, FriendshipStateDto, RecordFriendshipEventResult,
};
use crate::error::AppError;
use crate::repositories::friendship_repository::FriendshipRepository;

#[derive(Debug, Clone)]
pub struct FriendshipService {
    friendship_repository: FriendshipRepository,
}

impl FriendshipService {
    pub fn new(friendship_repository: FriendshipRepository) -> Self {
        Self {
            friendship_repository,
        }
    }

    /// 初回起動時に既定状態を保存する（未保存なら）。
    pub fn initialize_default_if_missing(&self) -> Result<(), AppError> {
        if !self.friendship_repository.exists() {
            let state = self.friendship_repository.load_or_default()?;
            self.friendship_repository.save(&state)?;
        }
        Ok(())
    }

    /// 現在の友情ランク状態（読み取り）。
    pub fn get_friendship_state(&self) -> Result<FriendshipStateDto, AppError> {
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
}

/// 当日の日付（UTC・"YYYY-MM-DD"）。デイリーリセット判定に使う。
fn current_utc_date() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

/// 現在時刻（UTC・"YYYY-MM-DDThh:mm:ssZ"）。ランクアップ時刻記録に使う。
fn current_utc_timestamp() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}
