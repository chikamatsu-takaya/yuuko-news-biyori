//! 友情ランクのドメイン。
//!
//! - 読み取り用DTO `FriendshipStateDto`（`get_friendship_state` が返す形）。
//! - 永続化用 `FriendshipState`（`user/friendship.json`・データ設計書 §10.5 準拠）。
//! - ポイント加算イベント `FriendshipEventType`（§10.4）と、加算・ランクアップ判定の純ロジック。
//!
//! 簡易仕様（友情ランク簡易完成）:
//! - ランクアップは flat `POINTS_PER_RANK`(100) pt/ランク・`RANK_MAX`(20) まで。
//! - デイリー上限はポイントで頭打ち（既定 `DEFAULT_DAILY_LIMIT` = 25）。**上限はRust側で強制**する。
//! - 報酬カタログ未整備のため、ランクアップは演出のみ。`pending_reward_ids` は将来用に予約（今回は空運用）。
//!
//! 時刻依存（今日の日付・ランクアップ時刻）は引数で受け取り、本モジュールは純粋に保つ（テスト容易化）。

use serde::{Deserialize, Serialize};

/// 1ランクアップに必要なポイント（簡易・flat）。
pub const POINTS_PER_RANK: u32 = 100;
/// 最大ランク。
pub const RANK_MAX: u32 = 20;
/// デイリー加算上限の既定値（pt）。
pub const DEFAULT_DAILY_LIMIT: u32 = 25;

/// 友情ポイント加算イベント（データ設計書 §10.4）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FriendshipEventType {
    YuukoToMain,
    NewsDetailOpened,
    ExplanationViewed,
    TermExplained,
}

impl FriendshipEventType {
    /// 保存/フロントと一致する文字列から復元する。未知の値は `None`（＝無効イベント）。
    pub fn from_storage(value: &str) -> Option<Self> {
        match value {
            "yuuko_to_main" => Some(Self::YuukoToMain),
            "news_detail_opened" => Some(Self::NewsDetailOpened),
            "explanation_viewed" => Some(Self::ExplanationViewed),
            "term_explained" => Some(Self::TermExplained),
            _ => None,
        }
    }

    /// 加算ポイント（§10.4）。
    pub fn points(self) -> u32 {
        match self {
            Self::YuukoToMain => 5,
            Self::NewsDetailOpened => 3,
            Self::ExplanationViewed => 4,
            Self::TermExplained => 1,
        }
    }
}

/// デイリー加算状況（§10.5）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyPoints {
    pub date: String,
    pub points: u32,
    pub limit: u32,
}

impl Default for DailyPoints {
    fn default() -> Self {
        Self {
            date: String::new(),
            points: 0,
            limit: DEFAULT_DAILY_LIMIT,
        }
    }
}

/// 永続化する友情ランク状態（`user/friendship.json`・§10.5）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FriendshipState {
    pub version: u32,
    pub current_rank: u32,
    pub current_points: u32,
    pub total_points: u32,
    pub daily_points: DailyPoints,
    pub rank_max: u32,
    #[serde(default)]
    pub last_rank_up_at: Option<String>,
    #[serde(default)]
    pub pending_reward_ids: Vec<String>,
    #[serde(default)]
    pub confirmed_reward_ids: Vec<String>,
}

impl Default for FriendshipState {
    fn default() -> Self {
        Self {
            version: 1,
            current_rank: 0,
            current_points: 0,
            total_points: 0,
            daily_points: DailyPoints::default(),
            rank_max: RANK_MAX,
            last_rank_up_at: None,
            pending_reward_ids: Vec::new(),
            confirmed_reward_ids: Vec::new(),
        }
    }
}

/// ポイント加算結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EarnOutcome {
    /// 実際に加算されたポイント（デイリー上限により 0 のこともある）。
    pub earned_points: u32,
    /// この加算でランクアップしたか。
    pub ranked_up: bool,
}

impl FriendshipState {
    /// イベントによるポイント加算（デイリー上限で頭打ち・ランクアップ判定込み）。
    /// `today` は当日の日付（"YYYY-MM-DD"）、`now_rfc3339` はランクアップ時刻記録用。
    pub fn earn(
        &mut self,
        event: FriendshipEventType,
        today: &str,
        now_rfc3339: &str,
    ) -> EarnOutcome {
        // 日付が変わっていればデイリーをリセットする。
        if self.daily_points.date != today {
            self.daily_points.date = today.to_string();
            self.daily_points.points = 0;
        }

        // デイリー上限までの残り。0なら加算しない。
        let remaining = self
            .daily_points
            .limit
            .saturating_sub(self.daily_points.points);
        let add = event.points().min(remaining);
        if add == 0 {
            return EarnOutcome {
                earned_points: 0,
                ranked_up: false,
            };
        }

        self.daily_points.points += add;
        self.total_points += add;
        self.current_points += add;

        // ランクアップ判定（flat POINTS_PER_RANK・RANK_MAX まで）。
        let mut ranked_up = false;
        while self.current_rank < self.rank_max && self.current_points >= POINTS_PER_RANK {
            self.current_points -= POINTS_PER_RANK;
            self.current_rank += 1;
            self.last_rank_up_at = Some(now_rfc3339.to_string());
            ranked_up = true;
        }

        // 上限ランク到達後は端数を持ち越さない（簡易仕様）。
        if self.current_rank >= self.rank_max {
            self.current_points = 0;
        }

        EarnOutcome {
            earned_points: add,
            ranked_up,
        }
    }

    /// 次ランクまでに必要なポイント（DTO表示用）。上限ランクでは 0。
    pub fn next_required_point(&self) -> u32 {
        if self.current_rank >= self.rank_max {
            0
        } else {
            POINTS_PER_RANK
        }
    }

    /// 読み取り用DTOへ変換する。
    pub fn to_dto(&self) -> FriendshipStateDto {
        FriendshipStateDto {
            current_rank: self.current_rank,
            current_point: self.current_points,
            next_required_point: self.next_required_point(),
            daily_earned_point: self.daily_points.points,
            daily_point_limit: self.daily_points.limit,
            last_point_date: if self.daily_points.date.is_empty() {
                None
            } else {
                Some(self.daily_points.date.clone())
            },
        }
    }
}

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
    /// 未保存時の既定値（rank0・point0・しきい値100・デイリー上限は既定）。
    fn default() -> Self {
        FriendshipState::default().to_dto()
    }
}

/// `record_friendship_event` の入力（フロントからのイベント通知）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordFriendshipEventParams {
    /// イベント種別（`yuuko_to_main` / `news_detail_opened` / `explanation_viewed` / `term_explained`）。
    pub event_type: String,
}

/// `record_friendship_event` の結果（更新後の状態＋ランクアップ有無）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordFriendshipEventResult {
    pub state: FriendshipStateDto,
    /// この加算でランクアップしたか（フロントは true の時に RankUpDialog を表示）。
    pub ranked_up: bool,
    pub new_rank: u32,
    /// 実際に加算されたポイント（デイリー上限により 0 のこともある）。
    pub earned_point: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_friendship_state_is_zero_progress() {
        let state = FriendshipStateDto::default();
        assert_eq!(state.current_rank, 0);
        assert_eq!(state.current_point, 0);
        assert_eq!(state.next_required_point, POINTS_PER_RANK);
        assert_eq!(state.daily_point_limit, DEFAULT_DAILY_LIMIT);
        assert!(state.last_point_date.is_none());
    }

    #[test]
    fn event_type_parses_and_scores() {
        assert_eq!(
            FriendshipEventType::from_storage("yuuko_to_main"),
            Some(FriendshipEventType::YuukoToMain)
        );
        assert_eq!(FriendshipEventType::YuukoToMain.points(), 5);
        assert_eq!(FriendshipEventType::NewsDetailOpened.points(), 3);
        assert_eq!(FriendshipEventType::ExplanationViewed.points(), 4);
        assert_eq!(FriendshipEventType::TermExplained.points(), 1);
        assert_eq!(FriendshipEventType::from_storage("unknown"), None);
    }

    #[test]
    fn earn_adds_points_within_daily_limit() {
        let mut state = FriendshipState::default();
        let out = state.earn(FriendshipEventType::NewsDetailOpened, "2026-06-08", "t");
        assert_eq!(out.earned_points, 3);
        assert!(!out.ranked_up);
        assert_eq!(state.current_points, 3);
        assert_eq!(state.total_points, 3);
        assert_eq!(state.daily_points.points, 3);
        assert_eq!(state.daily_points.date, "2026-06-08");
    }

    #[test]
    fn earn_is_capped_by_daily_limit() {
        let mut state = FriendshipState::default();
        // 上限25。yuuko_to_main(5) を 5回で 25 に到達、6回目は加算されない。
        for _ in 0..5 {
            state.earn(FriendshipEventType::YuukoToMain, "2026-06-08", "t");
        }
        assert_eq!(state.daily_points.points, 25);
        let out = state.earn(FriendshipEventType::YuukoToMain, "2026-06-08", "t");
        assert_eq!(out.earned_points, 0);
        assert_eq!(state.daily_points.points, 25);
        assert_eq!(state.total_points, 25);
    }

    #[test]
    fn earn_partial_when_near_daily_limit() {
        // 24ptまで貯め（残り1）、4ptイベントは1ptだけ入る。
        let mut state = FriendshipState {
            daily_points: DailyPoints {
                date: "2026-06-08".to_string(),
                points: 24,
                limit: DEFAULT_DAILY_LIMIT,
            },
            current_points: 24,
            total_points: 24,
            ..FriendshipState::default()
        };
        let out = state.earn(FriendshipEventType::ExplanationViewed, "2026-06-08", "t");
        assert_eq!(out.earned_points, 1);
        assert_eq!(state.daily_points.points, 25);
    }

    #[test]
    fn daily_points_reset_on_new_date() {
        let mut state = FriendshipState::default();
        state.earn(FriendshipEventType::YuukoToMain, "2026-06-08", "t"); // 5
        assert_eq!(state.daily_points.points, 5);
        // 翌日：デイリーはリセットされ、累計は維持。
        let out = state.earn(FriendshipEventType::YuukoToMain, "2026-06-09", "t");
        assert_eq!(out.earned_points, 5);
        assert_eq!(state.daily_points.date, "2026-06-09");
        assert_eq!(state.daily_points.points, 5);
        assert_eq!(state.total_points, 10);
    }

    #[test]
    fn rank_up_when_crossing_threshold() {
        // デイリー上限を一時的に大きくして100pt到達を試験。
        let mut state = FriendshipState {
            daily_points: DailyPoints {
                date: String::new(),
                points: 0,
                limit: 1000,
            },
            ..FriendshipState::default()
        };
        let mut ranked = false;
        for _ in 0..20 {
            // yuuko_to_main(5) × 20 = 100 で 1ランクアップ。
            let out = state.earn(
                FriendshipEventType::YuukoToMain,
                "2026-06-08",
                "2026-06-08T00:00:00Z",
            );
            ranked = ranked || out.ranked_up;
        }
        assert_eq!(state.current_rank, 1);
        assert_eq!(state.current_points, 0);
        assert!(ranked);
        assert_eq!(
            state.last_rank_up_at.as_deref(),
            Some("2026-06-08T00:00:00Z")
        );
    }

    #[test]
    fn rank_does_not_exceed_rank_max() {
        let mut state = FriendshipState {
            current_rank: RANK_MAX,
            daily_points: DailyPoints {
                date: String::new(),
                points: 0,
                limit: 1000,
            },
            ..FriendshipState::default()
        };
        let out = state.earn(FriendshipEventType::YuukoToMain, "2026-06-08", "t");
        // 上限ランクではランクアップせず、端数も持ち越さない。
        assert!(!out.ranked_up);
        assert_eq!(state.current_rank, RANK_MAX);
        assert_eq!(state.current_points, 0);
        assert_eq!(state.next_required_point(), 0);
    }
}
