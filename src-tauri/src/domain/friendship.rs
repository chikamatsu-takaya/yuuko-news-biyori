//! 友情ランクの読み取り用ドメイン。
//!
//! 本PRでは `get_friendship_state` が返す**読み取り専用DTOの形のみ**を定義する。
//! 永続化（`user/friendship.json`）・ポイント加算・ランクアップ判定・RankUpDialog 配線は
//! 後続「友情ランク簡易完成」PRで実装する。

use serde::{Deserialize, Serialize};

/// 友情ランク状態の読み取り用DTO（データ設計書 §8.4 準拠の最小集合）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FriendshipStateDto {
    pub current_rank: u32,
    pub current_point: u32,
    pub next_required_point: u32,
    pub daily_earned_point: u32,
    pub daily_point_limit: u32,
    pub last_point_date: Option<String>,
}

impl Default for FriendshipStateDto {
    /// 暫定の既定値。実際のしきい値・加算ロジックは後続PRで定義する。
    fn default() -> Self {
        Self {
            current_rank: 0,
            current_point: 0,
            next_required_point: 100,
            daily_earned_point: 0,
            daily_point_limit: 30,
            last_point_date: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_friendship_state_is_zero_progress() {
        let state = FriendshipStateDto::default();
        assert_eq!(state.current_rank, 0);
        assert_eq!(state.current_point, 0);
        assert!(state.last_point_date.is_none());
    }
}
