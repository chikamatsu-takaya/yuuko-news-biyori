use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::article::{ArticleReadState, ArticleSummaryDto};
use crate::error::AppError;

/// 通知ゲートの時間しきい値（分）。設計書 §6.2「通知頻度制御」。将来は設定化可能とする。
const MIN_COOLTIME_MINUTES: i64 = 60; // 前回通知からの最短間隔
const DISMISS_COOLDOWN_MINUTES: i64 = 120; // ユーザーが閉じた後の再通知抑制
const IGNORE_COOLDOWN_MINUTES: i64 = 180; // 無操作（無視）後の再通知抑制
/// 紹介済みIDの保持上限（FIFO）。状態ファイルの肥大化を防ぐ。
const INTRODUCED_HISTORY_CAP: usize = 500;
/// 保存用タイムスタンプ形式（UTC）。news / friendship / summary と統一。
const TIMESTAMP_FORMAT: &str = "%Y-%m-%dT%H:%M:%SZ";
const DATE_FORMAT: &str = "%Y-%m-%d";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "PascalCase")]
pub enum YuukoResidentState {
    Hidden,
    #[default]
    Waiting,
    Suppressed,
    Preparing,
    Appearing,
    BalloonVisible,
    PreviewVisible,
    Leaving,
    TransitionPending,
    RewardNotifying,
    Paused,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "PascalCase")]
pub enum YuukoPositionMode {
    #[default]
    RightBottom,
    LeftBottom,
    RightCenter,
    LeftCenter,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct RewardNotificationState {
    pub pending: bool,
    pub rank: u32,
    pub reward_ids: Vec<String>,
    pub message: String,
}

/// 当日の通知回数（日次上限判定用）。日付が変わると count をリセットする。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct DailyNotificationCount {
    pub date: String,
    pub count: u32,
}

/// 通知ゲートの判定結果（内部用・非永続）。設計書 §4.3/§5.2 のMVP抑制条件に対応。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationGate {
    Allowed,
    DailyLimitReached,
    CoolingDown,
    OutsideTimeRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct PersistedYuukoState {
    pub state: YuukoResidentState,
    pub position_mode: YuukoPositionMode,
    pub balloon_text: Option<String>,
    pub preview_article: Option<ArticleSummaryDto>,
    pub current_article_id: Option<String>,
    pub reward_notification: Option<RewardNotificationState>,
    pub confirmed_reward_ids: Vec<String>,
    /// 前回通知時刻（UTC・RFC3339）。最短クールタイム判定の基点。
    pub last_notified_at: Option<String>,
    /// 再通知抑制の終端時刻（UTC・RFC3339）。閉じる/無視で設定する。
    pub cooldown_until: Option<String>,
    /// 当日の通知回数（日次上限判定用）。
    pub daily_notification: DailyNotificationCount,
    /// ゆうこが紹介済みの記事ID（FIFO・上限キャップ）。再紹介の抑止に使う。
    pub introduced_article_ids: Vec<String>,
}

impl Default for PersistedYuukoState {
    fn default() -> Self {
        Self {
            state: YuukoResidentState::Waiting,
            position_mode: YuukoPositionMode::RightBottom,
            balloon_text: Some("今日もニュースを見つけたら声をかけるね。".to_string()),
            preview_article: None,
            current_article_id: None,
            reward_notification: None,
            confirmed_reward_ids: Vec::new(),
            last_notified_at: None,
            cooldown_until: None,
            daily_notification: DailyNotificationCount::default(),
            introduced_article_ids: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct YuukoNotificationState {
    pub state: YuukoResidentState,
    pub position_mode: YuukoPositionMode,
    pub balloon_text: Option<String>,
    pub preview_article: Option<ArticleSummaryDto>,
    pub has_notification: bool,
    pub current_article_id: Option<String>,
    pub reward_notification: Option<RewardNotificationState>,
}

impl PersistedYuukoState {
    pub fn to_notification_state(&self) -> YuukoNotificationState {
        let has_reward_notification = self
            .reward_notification
            .as_ref()
            .is_some_and(|reward| reward.pending);

        let state = if has_reward_notification {
            YuukoResidentState::RewardNotifying
        } else {
            self.state
        };

        YuukoNotificationState {
            state,
            position_mode: self.position_mode,
            balloon_text: self.balloon_text.clone(),
            preview_article: self.preview_article.clone(),
            has_notification: has_reward_notification,
            current_article_id: self.current_article_id.clone(),
            reward_notification: self.reward_notification.clone(),
        }
    }

    pub fn confirm_rank_up_reward(
        &mut self,
        reward_ids: &[String],
    ) -> Result<ConfirmRankUpRewardResult, AppError> {
        if reward_ids.is_empty() {
            return Err(AppError::Validation(
                "rewardIds must contain at least one item".to_string(),
            ));
        }

        let reward_notification = self.reward_notification.as_mut().ok_or_else(|| {
            AppError::Validation("no pending reward notification exists".to_string())
        })?;

        let mut confirmed_reward_ids = Vec::new();
        for reward_id in reward_ids {
            if reward_id.trim().is_empty() {
                return Err(AppError::Validation(
                    "rewardIds must not contain empty values".to_string(),
                ));
            }

            if let Some(index) = reward_notification
                .reward_ids
                .iter()
                .position(|pending_id| pending_id == reward_id)
            {
                reward_notification.reward_ids.remove(index);
                confirmed_reward_ids.push(reward_id.clone());
            }
        }

        if confirmed_reward_ids.is_empty() {
            return Err(AppError::Validation(
                "none of rewardIds matched pending rewards".to_string(),
            ));
        }

        for reward_id in &confirmed_reward_ids {
            if !self.confirmed_reward_ids.iter().any(|id| id == reward_id) {
                self.confirmed_reward_ids.push(reward_id.clone());
            }
        }

        reward_notification.pending = !reward_notification.reward_ids.is_empty();
        let remaining_pending_reward_ids = reward_notification.reward_ids.clone();

        if !reward_notification.pending {
            self.reward_notification = None;
            self.state = YuukoResidentState::Waiting;
        }

        Ok(ConfirmRankUpRewardResult {
            ok: true,
            confirmed_reward_ids,
            remaining_pending_reward_ids,
        })
    }

    /// 通知を閉じる。balloon / preview / current_article をクリアし Waiting に戻し、
    /// 再通知抑制（クールタイム）を設定する（設計書 §6.2/§10.6「閉じる→クールタイム設定」）。
    /// pending な reward_notification と confirmed_reward_ids は保持する
    /// （報酬確認は confirm_rank_up_reward が担当するため、ここでは消さない）。
    pub fn dismiss_notification(&mut self, now: DateTime<Utc>) {
        // アクティブな通知が無い状態（Waiting等）での誤呼び出しは no-op。
        // 不要なクールダウンで通知が長時間ブロックされるのを防ぐ。
        if !self.has_active_notification() {
            return;
        }
        self.clear_active_notification();
        self.cooldown_until = Some(format_timestamp(
            now + Duration::minutes(DISMISS_COOLDOWN_MINUTES),
        ));
    }

    /// 無操作タイムアウト（無視）。閉じるより長い再通知抑制を設定する（設計書 §6.2/§10.6）。
    /// reward は保持する。フロントの自動退場タイマーから呼ぶ想定（Rustはタイマーを持たない）。
    pub fn mark_ignored(&mut self, now: DateTime<Utc>) {
        // アクティブな通知が無ければ no-op（誤クールダウン防止）。
        if !self.has_active_notification() {
            return;
        }
        self.clear_active_notification();
        self.cooldown_until = Some(format_timestamp(
            now + Duration::minutes(IGNORE_COOLDOWN_MINUTES),
        ));
    }

    /// ニュース通知（紹介）が表示中でユーザー操作待ちかどうか。
    /// reward 通知（RewardNotifying）は confirm_rank_up_reward が扱うため含めない。
    pub fn has_active_notification(&self) -> bool {
        matches!(
            self.state,
            YuukoResidentState::Appearing
                | YuukoResidentState::BalloonVisible
                | YuukoResidentState::PreviewVisible
        )
    }

    /// アクティブな通知表示をクリアして待機へ戻す（reward は保持）。
    fn clear_active_notification(&mut self) {
        self.balloon_text = None;
        self.preview_article = None;
        self.current_article_id = None;
        self.state = YuukoResidentState::Waiting;
    }

    /// ゆうこが紹介済みの記事か。
    pub fn is_introduced(&self, article_id: &str) -> bool {
        self.introduced_article_ids
            .iter()
            .any(|id| id == article_id)
    }

    /// 紹介候補の適格判定: 未紹介かつ非お気に入り（設計書 §12.4 お気に入りは紹介不要）。
    fn is_eligible_candidate(&self, article: &ArticleSummaryDto) -> bool {
        !article.is_favorite && !self.is_introduced(&article.article_id)
    }

    /// 通知を出してよいか判定する（設計書 §4.3/§5.2 のMVP抑制条件）。
    /// 日次上限・閉じる/無視クールダウン・前回通知からの最短クールタイムを確認する。
    /// 時間帯判定（start_time, end_time）が指定されている場合は、ローカル時刻で判定する。
    /// notification.enabled と報酬優先は呼び出し側（service）が判定する。
    pub fn can_notify<'a>(
        &self,
        now: DateTime<Utc>,
        max_per_day: u32,
        time_ranges: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> NotificationGate {
        let local_time = now.with_timezone(&chrono::Local);
        use chrono::Timelike;
        let current_minutes = local_time.hour() * 60 + local_time.minute();

        if !is_within_any_notification_time_range(current_minutes, time_ranges) {
            return NotificationGate::OutsideTimeRange;
        }

        let today = now.format(DATE_FORMAT).to_string();
        let used_today = if self.daily_notification.date == today {
            self.daily_notification.count
        } else {
            0
        };
        if used_today >= max_per_day {
            return NotificationGate::DailyLimitReached;
        }

        if let Some(until) = self.cooldown_until.as_deref().and_then(parse_timestamp) {
            if now < until {
                return NotificationGate::CoolingDown;
            }
        }

        if let Some(last) = self.last_notified_at.as_deref().and_then(parse_timestamp) {
            if now < last + Duration::minutes(MIN_COOLTIME_MINUTES) {
                return NotificationGate::CoolingDown;
            }
        }

        NotificationGate::Allowed
    }

    /// おすすめ候補（スコア順）から紹介する1件を選ぶ。未紹介・非お気に入りを対象に未読を優先する
    /// （設計書 §12.2/§12.4）。候補が無ければ None。
    pub fn pick_introducible(&self, candidates: &[ArticleSummaryDto]) -> Option<ArticleSummaryDto> {
        if let Some(article) = candidates
            .iter()
            .find(|&a| a.read_state == ArticleReadState::Unread && self.is_eligible_candidate(a))
        {
            return Some(article.clone());
        }

        candidates
            .iter()
            .find(|&a| self.is_eligible_candidate(a))
            .cloned()
    }

    /// 選んだ記事を「ゆうこが紹介中」の状態にし、通知回数・クールタイム基点・紹介済みを記録する。
    /// 日付が変わっていれば日次カウントをリセットしてから加算する。
    pub fn mark_notified(&mut self, now: DateTime<Utc>, article: ArticleSummaryDto) {
        let today = now.format(DATE_FORMAT).to_string();
        if self.daily_notification.date != today {
            self.daily_notification.date = today;
            self.daily_notification.count = 0;
        }
        self.daily_notification.count = self.daily_notification.count.saturating_add(1);
        self.last_notified_at = Some(format_timestamp(now));

        if !self.is_introduced(&article.article_id) {
            self.introduced_article_ids.push(article.article_id.clone());
            if self.introduced_article_ids.len() > INTRODUCED_HISTORY_CAP {
                let overflow = self.introduced_article_ids.len() - INTRODUCED_HISTORY_CAP;
                self.introduced_article_ids.drain(0..overflow);
            }
        }

        self.balloon_text = Some(format!(
            "気になるニュースを見つけたよ。「{}」",
            article.title
        ));
        self.current_article_id = Some(article.article_id.clone());
        self.preview_article = Some(article);
        self.state = YuukoResidentState::BalloonVisible;
    }

    /// 2段階クリックの最小遷移。操作対象（preview_article / current_article_id）が
    /// 無ければ no-op（安全側）。初回クリック → PreviewVisible、
    /// PreviewVisible での再クリック → Leaving（確定）。
    /// 記事既読・画面遷移・クールタイム等の副作用は持たない。
    /// 戻り値は状態遷移が起きたか。
    pub fn handle_click(&mut self) -> bool {
        if self.preview_article.is_none() && self.current_article_id.is_none() {
            return false;
        }

        self.state = match self.state {
            YuukoResidentState::PreviewVisible => YuukoResidentState::Leaving,
            _ => YuukoResidentState::PreviewVisible,
        };
        true
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmRankUpRewardParams {
    pub reward_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmRankUpRewardResult {
    pub ok: bool,
    pub confirmed_reward_ids: Vec<String>,
    pub remaining_pending_reward_ids: Vec<String>,
}

/// `request_yuuko_notification` の結果。通知が出たか・理由・最新状態を返す。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestYuukoNotificationResult {
    pub notified: bool,
    /// "notified" / "disabled" / "reward_pending" / "already_active" / "daily_limit" / "cooling_down" / "no_candidate"
    pub reason: String,
    pub state: YuukoNotificationState,
}

/// 保存用タイムスタンプ文字列を生成する（UTC・他サービスと同形式）。
fn format_timestamp(value: DateTime<Utc>) -> String {
    value.format(TIMESTAMP_FORMAT).to_string()
}

/// 保存済みRFC3339文字列を UTC DateTime へ復元する。
/// パース不能ならその制約は無視（fail-open）し、恒久的に通知が止まらないようにする。
fn parse_timestamp(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// "HH:MM" 形式の文字列をパースして (時, 分) を返す。
fn parse_hour_minute(s: &str) -> Option<(u32, u32)> {
    let (h_str, m_str) = s.split_once(':')?;
    let h: u32 = h_str.parse().ok()?;
    let m: u32 = m_str.parse().ok()?;
    if h < 24 && m < 60 {
        Some((h, m))
    } else {
        None
    }
}

fn is_within_notification_time_range(
    current_minutes: u32,
    start_time: &str,
    end_time: &str,
) -> bool {
    if current_minutes >= 24 * 60 {
        return false;
    }

    let (start_h, start_m) = match parse_hour_minute(start_time) {
        Some(time) => time,
        None => return false,
    };
    let (end_h, end_m) = match parse_hour_minute(end_time) {
        Some(time) => time,
        None => return false,
    };

    let start_minutes = start_h * 60 + start_m;
    let end_minutes = end_h * 60 + end_m;

    if start_minutes == end_minutes {
        return false;
    }

    if start_minutes < end_minutes {
        current_minutes >= start_minutes && current_minutes <= end_minutes
    } else {
        current_minutes >= start_minutes || current_minutes <= end_minutes
    }
}

fn is_within_any_notification_time_range<'a>(
    current_minutes: u32,
    time_ranges: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> bool {
    time_ranges.into_iter().any(|(start_time, end_time)| {
        is_within_notification_time_range(current_minutes, start_time, end_time)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::settings::PersistedSettings;
    use chrono::{Local, TimeZone};

    fn article(
        id: &str,
        read_state: ArticleReadState,
        is_favorite: bool,
        score: f32,
    ) -> ArticleSummaryDto {
        ArticleSummaryDto {
            article_id: id.to_string(),
            title: format!("記事 {id}"),
            source_name: "Example".to_string(),
            published_at_text: "2026-06-09".to_string(),
            genre: "AI".to_string(),
            summary: None,
            is_favorite,
            read_state,
            recommendation_score: score,
        }
    }

    fn state_with_notification() -> PersistedYuukoState {
        PersistedYuukoState {
            state: YuukoResidentState::BalloonVisible,
            position_mode: YuukoPositionMode::RightBottom,
            balloon_text: Some("気になるニュースがあるよ".to_string()),
            preview_article: None,
            current_article_id: Some("article-001".to_string()),
            reward_notification: Some(RewardNotificationState {
                pending: true,
                rank: 2,
                reward_ids: vec!["reward-1".to_string()],
                message: "ランクアップ！".to_string(),
            }),
            confirmed_reward_ids: vec!["reward-0".to_string()],
            ..PersistedYuukoState::default()
        }
    }

    #[test]
    fn dismiss_clears_active_notification_keeps_rewards_and_sets_cooldown() {
        let mut state = state_with_notification();
        let now = Utc.with_ymd_and_hms(2026, 6, 9, 12, 0, 0).unwrap();
        state.dismiss_notification(now);

        assert_eq!(state.state, YuukoResidentState::Waiting);
        assert!(state.balloon_text.is_none());
        assert!(state.preview_article.is_none());
        assert!(state.current_article_id.is_none());
        // 報酬は維持（confirm_rank_up_reward が担当）
        assert!(state.reward_notification.is_some());
        assert_eq!(state.confirmed_reward_ids, vec!["reward-0".to_string()]);
        // 閉じた後は再通知抑制（クールダウン）が設定される。
        assert!(state.cooldown_until.is_some());
        assert_eq!(
            state.can_notify(now, 3, [("00:00", "23:59")]),
            NotificationGate::CoolingDown
        );
    }

    #[test]
    fn handle_click_is_noop_without_preview_or_article() {
        let mut state = PersistedYuukoState {
            preview_article: None,
            current_article_id: None,
            ..PersistedYuukoState::default()
        };
        let initial_state = state.state;
        let transitioned = state.handle_click();
        assert!(!transitioned);
        assert_eq!(state.state, initial_state);
    }

    #[test]
    fn handle_click_advances_two_stages_when_content_present() {
        let mut state = state_with_notification();
        // 初回クリック → 軽量プレビュー
        assert!(state.handle_click());
        assert_eq!(state.state, YuukoResidentState::PreviewVisible);
        // 再クリック → 確定（退場）
        assert!(state.handle_click());
        assert_eq!(state.state, YuukoResidentState::Leaving);
    }

    #[test]
    fn can_notify_blocks_when_daily_limit_reached_and_resets_next_day() {
        let now = Utc.with_ymd_and_hms(2026, 6, 9, 12, 0, 0).unwrap();
        let state = PersistedYuukoState {
            daily_notification: DailyNotificationCount {
                date: "2026-06-09".to_string(),
                count: 3,
            },
            ..PersistedYuukoState::default()
        };
        assert_eq!(
            state.can_notify(now, 3, [("00:00", "23:59")]),
            NotificationGate::DailyLimitReached
        );
        // 翌日は日次カウントがリセットされ通知可能。
        let tomorrow = Utc.with_ymd_and_hms(2026, 6, 10, 9, 0, 0).unwrap();
        assert_eq!(
            state.can_notify(tomorrow, 3, [("00:00", "23:59")]),
            NotificationGate::Allowed
        );
    }

    #[test]
    fn can_notify_respects_min_cooltime() {
        let base = Utc.with_ymd_and_hms(2026, 6, 9, 12, 0, 0).unwrap();
        let mut state = PersistedYuukoState::default();
        state.mark_notified(base, article("a1", ArticleReadState::Unread, false, 0.9));
        // 直後はクールタイム中。
        assert_eq!(
            state.can_notify(base, 3, [("00:00", "23:59")]),
            NotificationGate::CoolingDown
        );
        // 最短クールタイム経過後は通知可能。
        let after = base + Duration::minutes(MIN_COOLTIME_MINUTES);
        assert_eq!(
            state.can_notify(after, 3, [("00:00", "23:59")]),
            NotificationGate::Allowed
        );
    }

    #[test]
    fn mark_notified_records_count_cooltime_and_introduced() {
        let now = Utc.with_ymd_and_hms(2026, 6, 9, 12, 0, 0).unwrap();
        let mut state = PersistedYuukoState::default();
        state.mark_notified(now, article("a1", ArticleReadState::Unread, false, 0.9));

        assert_eq!(state.daily_notification.count, 1);
        assert_eq!(state.daily_notification.date, "2026-06-09");
        assert!(state.last_notified_at.is_some());
        assert!(state.is_introduced("a1"));
        assert_eq!(state.current_article_id.as_deref(), Some("a1"));
        assert_eq!(state.state, YuukoResidentState::BalloonVisible);
        assert!(state.preview_article.is_some());
    }

    #[test]
    fn pick_introducible_prefers_unread_and_skips_introduced_and_favorite() {
        let state = PersistedYuukoState {
            introduced_article_ids: vec!["a1".to_string()],
            ..PersistedYuukoState::default()
        };
        let candidates = vec![
            article("a1", ArticleReadState::Unread, false, 0.95), // 紹介済み → 除外
            article("a2", ArticleReadState::DetailViewed, false, 0.90), // 既読
            article("a3", ArticleReadState::Unread, true, 0.85),  // お気に入り → 除外
            article("a4", ArticleReadState::Unread, false, 0.80), // ← 未読・適格で最優先
        ];
        let chosen = state.pick_introducible(&candidates).unwrap();
        assert_eq!(chosen.article_id, "a4");
    }

    #[test]
    fn pick_introducible_falls_back_to_read_when_no_unread() {
        let state = PersistedYuukoState::default();
        let candidates = vec![
            article("a1", ArticleReadState::DetailViewed, false, 0.90),
            article("a2", ArticleReadState::Previewed, true, 0.80), // お気に入り → 除外
        ];
        let chosen = state.pick_introducible(&candidates).unwrap();
        assert_eq!(chosen.article_id, "a1");
    }

    #[test]
    fn mark_ignored_keeps_reward_and_sets_longer_cooldown() {
        let now = Utc.with_ymd_and_hms(2026, 6, 9, 12, 0, 0).unwrap();
        let mut state = state_with_notification();
        state.mark_ignored(now);

        assert_eq!(state.state, YuukoResidentState::Waiting);
        assert!(state.reward_notification.is_some()); // reward は保持
                                                      // 無視のクールダウンは閉じる(120分)より長い。120分後でもまだ抑制中。
        let after_dismiss_window = now + Duration::minutes(DISMISS_COOLDOWN_MINUTES);
        assert_eq!(
            state.can_notify(after_dismiss_window, 3, [("00:00", "23:59")]),
            NotificationGate::CoolingDown
        );
    }

    #[test]
    fn dismiss_and_ignore_are_noop_without_active_notification() {
        let now = Utc.with_ymd_and_hms(2026, 6, 9, 12, 0, 0).unwrap();
        // 既定は Waiting（アクティブ通知なし）。
        let mut state = PersistedYuukoState::default();
        assert!(!state.has_active_notification());

        state.dismiss_notification(now);
        assert!(state.cooldown_until.is_none());
        assert_eq!(
            state.can_notify(now, 3, [("00:00", "23:59")]),
            NotificationGate::Allowed
        );

        state.mark_ignored(now);
        assert!(state.cooldown_until.is_none());
        assert_eq!(
            state.can_notify(now, 3, [("00:00", "23:59")]),
            NotificationGate::Allowed
        );
    }

    #[test]
    fn time_range_check_accepts_normal_range_inside() {
        assert!(is_within_notification_time_range(12 * 60, "09:00", "18:00"));
    }

    #[test]
    fn time_range_check_rejects_normal_range_outside() {
        assert!(!is_within_notification_time_range(
            20 * 60,
            "09:00",
            "18:00"
        ));
    }

    #[test]
    fn time_range_check_accepts_overnight_range_late_night() {
        assert!(is_within_notification_time_range(23 * 60, "22:00", "07:00"));
    }

    #[test]
    fn time_range_check_accepts_overnight_range_early_morning() {
        assert!(is_within_notification_time_range(5 * 60, "22:00", "07:00"));
    }

    #[test]
    fn time_range_check_rejects_overnight_range_daytime() {
        assert!(!is_within_notification_time_range(
            12 * 60,
            "22:00",
            "07:00"
        ));
    }

    #[test]
    fn time_range_check_rejects_same_start_and_end_as_zero_length() {
        assert!(!is_within_notification_time_range(
            13 * 60,
            "13:00",
            "13:00"
        ));
        assert!(!is_within_notification_time_range(
            20 * 60,
            "13:00",
            "13:00"
        ));
    }

    #[test]
    fn time_range_check_rejects_invalid_time_format() {
        assert!(!is_within_notification_time_range(
            12 * 60,
            "invalid",
            "18:00"
        ));
    }

    #[test]
    fn any_time_range_check_accepts_second_matching_range() {
        let ranges = [("09:00", "12:00"), ("13:00", "18:00")];

        assert!(is_within_any_notification_time_range(14 * 60, ranges));
    }

    #[test]
    fn any_time_range_check_rejects_when_no_ranges_match() {
        let ranges = [("09:00", "12:00"), ("13:00", "18:00")];

        assert!(!is_within_any_notification_time_range(20 * 60, ranges));
    }

    #[test]
    fn any_time_range_check_rejects_zero_length_range_even_with_multiple_ranges() {
        let ranges = [("09:00", "12:00"), ("13:00", "13:00")];

        assert!(!is_within_any_notification_time_range(20 * 60, ranges));
    }

    #[test]
    fn any_time_range_check_keeps_valid_range_when_other_range_is_zero_length() {
        let ranges = [("09:00", "12:00"), ("13:00", "13:00")];

        assert!(is_within_any_notification_time_range(10 * 60, ranges));
    }

    #[test]
    fn default_work_ranges_block_local_lunch_break() {
        let local_lunch = Local.with_ymd_and_hms(2026, 6, 9, 12, 30, 0).unwrap();
        let now = local_lunch.with_timezone(&Utc);
        let state = PersistedYuukoState::default();
        let ranges = [("09:00", "12:00"), ("13:00", "18:00")];

        assert_eq!(
            state.can_notify(now, 3, ranges),
            NotificationGate::OutsideTimeRange
        );
    }

    #[test]
    fn default_work_ranges_allow_local_afternoon() {
        let local_afternoon = Local.with_ymd_and_hms(2026, 6, 9, 13, 30, 0).unwrap();
        let now = local_afternoon.with_timezone(&Utc);
        let state = PersistedYuukoState::default();
        let ranges = [("09:00", "12:00"), ("13:00", "18:00")];

        assert_eq!(state.can_notify(now, 3, ranges), NotificationGate::Allowed);
    }

    #[test]
    fn missing_work_ranges_settings_block_local_lunch_break() {
        let legacy = r#"{
            "version": 1,
            "notification": {
                "enabled": true,
                "mode": "random_in_work_time",
                "maxPerDay": 3
            }
        }"#;
        let settings: PersistedSettings =
            serde_json::from_str(legacy).expect("legacy settings should load");
        let local_lunch = Local.with_ymd_and_hms(2026, 6, 9, 12, 30, 0).unwrap();
        let now = local_lunch.with_timezone(&Utc);
        let state = PersistedYuukoState::default();

        assert_eq!(
            state.can_notify(
                now,
                settings.notification.max_per_day,
                settings
                    .notification
                    .work_time_ranges
                    .iter()
                    .map(|range| (range.start.as_str(), range.end.as_str())),
            ),
            NotificationGate::OutsideTimeRange
        );
    }

    #[test]
    fn missing_work_ranges_settings_allow_local_afternoon() {
        let legacy = r#"{
            "version": 1,
            "notification": {
                "enabled": true,
                "mode": "random_in_work_time",
                "maxPerDay": 3
            }
        }"#;
        let settings: PersistedSettings =
            serde_json::from_str(legacy).expect("legacy settings should load");
        let local_afternoon = Local.with_ymd_and_hms(2026, 6, 9, 13, 30, 0).unwrap();
        let now = local_afternoon.with_timezone(&Utc);
        let state = PersistedYuukoState::default();

        assert_eq!(
            state.can_notify(
                now,
                settings.notification.max_per_day,
                settings
                    .notification
                    .work_time_ranges
                    .iter()
                    .map(|range| (range.start.as_str(), range.end.as_str())),
            ),
            NotificationGate::Allowed
        );
    }
}
