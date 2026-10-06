//! 友情ランクのドメイン。
//!
//! - 読み取り用DTO `FriendshipStateDto`（`get_friendship_state` が返す形）。
//! - 永続化用 `FriendshipState`（`user/friendship.json`・データ設計書 §10.5 準拠）。
//! - ポイント加算イベント `FriendshipEventType`（§10.4）と、加算・ランクアップ判定の純ロジック。
//!
//! 仕様:
//! - ランクは 1 から始まり `RANK_MAX`(20) で止まる段階制（要件定義書 §7.6.1 / §7.6.7）。
//!   Rank r → r+1 の必要ポイントは `required_points_for_next_rank`（10pt から 5pt 刻みで 100pt）。
//! - ランクと「現ランク内の進捗ポイント」は**累計ポイント `total_points` から毎回導出**する。
//!   旧データ（ランク0開始・flat 100pt/ランク）も累計から計算し直せば、保存形式を変えずに新しい表へ移行できる。
//! - デイリー上限はポイントで頭打ち（既定 `DEFAULT_DAILY_LIMIT` = 25）。**上限はRust側で強制**する。
//! - 報酬カタログ未整備のため、ランクアップは演出のみ。`pending_reward_ids` は将来用に予約（今回は空運用）。
//!
//! 時刻依存（今日の日付・ランクアップ時刻）は引数で受け取り、本モジュールは純粋に保つ（テスト容易化）。

use serde::{Deserialize, Serialize};

/// 開始ランク（要件定義書 §7.6.7 の表が Rank 1 から始まるため）。
pub const RANK_MIN: u32 = 1;
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
            current_rank: RANK_MIN,
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

/// Rank `rank` → `rank + 1` に必要なポイント（要件定義書 §7.6.7）。
/// 前半は軽く後半はやや重く、Rank 1→2 の 10pt から 5pt 刻みで Rank 19→20 の 100pt まで。
/// 最大ランク以上（および範囲外）では 0（＝これ以上上がらない）。
pub fn required_points_for_next_rank(rank: u32) -> u32 {
    if (RANK_MIN..RANK_MAX).contains(&rank) {
        5 + 5 * rank
    } else {
        0
    }
}

/// 累計ポイントから (ランク, 現ランク内の進捗ポイント) を求める。
/// 最大ランク到達後は累計がいくら増えても `RANK_MAX` で止め、進捗は 0 とする
/// （次ランクが存在せず「次まで x/y」を表示できないため。従来の上限到達時と同じ扱い）。
pub fn rank_progress_from_total(total_points: u32) -> (u32, u32) {
    let mut rank = RANK_MIN;
    let mut remaining = total_points;
    while rank < RANK_MAX {
        let required = required_points_for_next_rank(rank);
        if remaining < required {
            return (rank, remaining);
        }
        remaining -= required;
        rank += 1;
    }
    (RANK_MAX, 0)
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
    /// 累計ポイントからランク・進捗を計算し直す（読み込み時に必ず通す）。
    ///
    /// 保存済みの `current_rank` / `current_points` は旧仕様（ランク0開始・flat 100pt）の値の
    /// 可能性があるため信用せず、正である `total_points` から導出する。保存形式は変えない。
    /// 旧仕様より各ランクの閾値が低いので、移行でランクが下がることはない。
    /// 移行によるランク上昇は「加算によるランクアップ」ではないため、ランクアップ演出・
    /// `last_rank_up_at`・報酬状態（`pending_reward_ids`）には触れない（黙って反映する）。
    pub fn normalize_rank_from_total(&mut self) {
        let (rank, progress) = rank_progress_from_total(self.total_points);
        self.current_rank = rank;
        self.current_points = progress;
        self.rank_max = RANK_MAX;
    }

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

        // 加算前のランクも累計から求める。保存値（旧仕様のランク）と比べると、
        // 移行によるランク差をこの加算のランクアップと誤判定してしまうため。
        let (rank_before, _) = rank_progress_from_total(self.total_points);

        self.daily_points.points += add;
        // 累計は最大ランク到達後も加算し続ける（ランクは RANK_MAX で止まる）。
        self.total_points = self.total_points.saturating_add(add);
        self.normalize_rank_from_total();

        let ranked_up = self.current_rank > rank_before;
        if ranked_up {
            self.last_rank_up_at = Some(now_rfc3339.to_string());
        }

        EarnOutcome {
            earned_points: add,
            ranked_up,
        }
    }

    /// 現ランクから次ランクへ上がるのに必要なポイント（DTO表示用・ランクごとに異なる）。
    /// `current_point` と組で「次のランクまで current_point / next_required_point」を表す。上限ランクでは 0。
    pub fn next_required_point(&self) -> u32 {
        required_points_for_next_rank(self.current_rank)
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
    /// 現在のランク（1〜20）。
    pub current_rank: u32,
    /// 現ランク内の進捗ポイント（ランクアップごとに 0 から数え直す）。
    pub current_point: u32,
    /// 現ランクから次ランクへの必要ポイント（ランクごとに異なる・最大ランクでは 0）。
    pub next_required_point: u32,
    pub daily_earned_point: u32,
    pub daily_point_limit: u32,
    pub last_point_date: Option<String>,
}

impl Default for FriendshipStateDto {
    /// 未保存時の既定値（rank1・point0・しきい値10・デイリー上限は既定）。
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
    fn default_friendship_state_starts_at_rank_one() {
        let state = FriendshipStateDto::default();
        assert_eq!(state.current_rank, 1);
        assert_eq!(state.current_point, 0);
        assert_eq!(state.next_required_point, 10);
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

    /// 各ランク到達に必要な累計ポイント（§7.6.7 の表の累積和）。
    fn cumulative_threshold(rank: u32) -> u32 {
        (RANK_MIN..rank).map(required_points_for_next_rank).sum()
    }

    /// デイリー上限を一時的に大きくし、累計 `total` の状態を作る（ランクは累計から導出）。
    fn state_with_total(total: u32) -> FriendshipState {
        let mut state = FriendshipState {
            total_points: total,
            daily_points: DailyPoints {
                date: "2026-06-08".to_string(),
                points: 0,
                limit: 10_000,
            },
            ..FriendshipState::default()
        };
        state.normalize_rank_from_total();
        state
    }

    #[test]
    fn required_points_follow_requirement_table() {
        // Rank1→2: 10pt から 5pt 刻みで Rank19→20: 100pt。
        let table: Vec<u32> = (RANK_MIN..RANK_MAX)
            .map(required_points_for_next_rank)
            .collect();
        let expected: Vec<u32> = (0..19).map(|i| 10 + 5 * i).collect();
        assert_eq!(table, expected);
        assert_eq!(required_points_for_next_rank(1), 10);
        assert_eq!(required_points_for_next_rank(19), 100);
        // 最大ランク・範囲外では 0。
        assert_eq!(required_points_for_next_rank(RANK_MAX), 0);
        assert_eq!(required_points_for_next_rank(0), 0);
        assert_eq!(required_points_for_next_rank(21), 0);
        // Rank20 到達に必要な累計は 1045pt。
        assert_eq!(cumulative_threshold(RANK_MAX), 1045);
    }

    #[test]
    fn rank_progress_at_every_threshold_boundary() {
        assert_eq!(rank_progress_from_total(0), (1, 0));
        for rank in (RANK_MIN + 1)..=RANK_MAX {
            let threshold = cumulative_threshold(rank);
            // 閾値ちょうどで到達（進捗0）し、1pt 手前では前ランクの最終ポイント。
            assert_eq!(rank_progress_from_total(threshold), (rank, 0));
            assert_eq!(
                rank_progress_from_total(threshold - 1),
                (rank - 1, required_points_for_next_rank(rank - 1) - 1)
            );
        }
        // 最大ランク到達後は累計が増えてもランク20・進捗0で止まる。
        assert_eq!(rank_progress_from_total(1046), (RANK_MAX, 0));
        assert_eq!(rank_progress_from_total(u32::MAX), (RANK_MAX, 0));
    }

    #[test]
    fn rank_up_exactly_at_threshold() {
        // 累計 9pt（Rank1・あと1pt）から 1pt でちょうど 10pt → Rank2。
        let mut state = state_with_total(9);
        assert_eq!(state.current_rank, 1);
        assert_eq!(state.current_points, 9);
        assert_eq!(state.next_required_point(), 10);

        let out = state.earn(
            FriendshipEventType::TermExplained,
            "2026-06-08",
            "2026-06-08T00:00:00Z",
        );
        assert!(out.ranked_up);
        assert_eq!(state.current_rank, 2);
        assert_eq!(state.current_points, 0);
        assert_eq!(state.total_points, 10);
        assert_eq!(state.next_required_point(), 15);
        assert_eq!(
            state.last_rank_up_at.as_deref(),
            Some("2026-06-08T00:00:00Z")
        );

        // 次の閾値（累計25）の手前ではランクアップしない。
        let mut state = state_with_total(20);
        let out = state.earn(FriendshipEventType::ExplanationViewed, "2026-06-08", "t");
        assert!(!out.ranked_up);
        assert_eq!((state.current_rank, state.current_points), (2, 14));
    }

    #[test]
    fn reaching_rank_max_and_beyond() {
        // 累計 1040（Rank19・95/100）から 5pt でちょうど Rank20。
        let mut state = state_with_total(1040);
        assert_eq!((state.current_rank, state.current_points), (19, 95));
        let out = state.earn(FriendshipEventType::YuukoToMain, "2026-06-08", "t");
        assert!(out.ranked_up);
        assert_eq!(state.current_rank, RANK_MAX);
        assert_eq!(state.current_points, 0);
        assert_eq!(state.next_required_point(), 0);

        // 最大ランク後も累計は加算されるが、ランクは上がらず進捗も持ち越さない。
        let out = state.earn(FriendshipEventType::YuukoToMain, "2026-06-08", "t2");
        assert_eq!(out.earned_points, 5);
        assert!(!out.ranked_up);
        assert_eq!(state.current_rank, RANK_MAX);
        assert_eq!(state.current_points, 0);
        assert_eq!(state.total_points, 1050);
        assert_eq!(state.last_rank_up_at.as_deref(), Some("t"));
        assert_eq!(state.to_dto().next_required_point, 0);
    }

    #[test]
    fn legacy_flat_state_is_recomputed_from_total_points() {
        // 旧仕様（ランク0開始・flat 100pt）で保存された状態: rank3・50pt・累計350。
        let json = r#"{
            "version": 1,
            "currentRank": 3,
            "currentPoints": 50,
            "totalPoints": 350,
            "dailyPoints": {"date": "2026-06-08", "points": 10, "limit": 25},
            "rankMax": 20,
            "lastRankUpAt": "2026-06-01T00:00:00Z",
            "pendingRewardIds": [],
            "confirmedRewardIds": ["deco_001"]
        }"#;
        let mut state: FriendshipState = serde_json::from_str(json).unwrap();
        state.normalize_rank_from_total();
        // 累計350 → Rank11（到達累計325）・進捗25/60。
        assert_eq!(state.current_rank, 11);
        assert_eq!(state.current_points, 25);
        assert_eq!(state.next_required_point(), 60);
        // 累計・日次・報酬・最終ランクアップ時刻は変えない（データを壊さない・演出を出さない）。
        assert_eq!(state.total_points, 350);
        assert_eq!(state.daily_points.points, 10);
        assert_eq!(state.confirmed_reward_ids, vec!["deco_001".to_string()]);
        assert!(state.pending_reward_ids.is_empty());
        assert_eq!(
            state.last_rank_up_at.as_deref(),
            Some("2026-06-01T00:00:00Z")
        );
    }

    #[test]
    fn legacy_jump_is_not_reported_as_rank_up_on_next_earn() {
        // 未正規化の旧状態（rank0・累計350）に直接加算しても、移行分のランク差は
        // この加算のランクアップとして扱わない（ダイアログを出さない）。
        let mut state = FriendshipState {
            current_rank: 0,
            current_points: 50,
            total_points: 350,
            ..FriendshipState::default()
        };
        let out = state.earn(FriendshipEventType::TermExplained, "2026-06-08", "t");
        assert!(!out.ranked_up);
        assert_eq!(state.current_rank, 11);
        assert_eq!(state.current_points, 26);
        assert!(state.last_rank_up_at.is_none());
    }
}
