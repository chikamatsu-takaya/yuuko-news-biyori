use chrono::Utc;

use crate::domain::article::GetRecommendedArticlesParams;
use crate::domain::yuuko::{
    ConfirmRankUpRewardParams, ConfirmRankUpRewardResult, NotificationGate, PersistedYuukoState,
    RequestYuukoNotificationResult, YuukoNotificationState, YuukoResidentState,
};
use crate::error::AppError;
use crate::repositories::settings_repository::SettingsRepository;
use crate::repositories::yuuko_state_repository::YuukoStateRepository;
use crate::services::article_service::ArticleService;

#[derive(Debug, Clone)]
pub struct YuukoService {
    settings_repository: SettingsRepository,
    yuuko_state_repository: YuukoStateRepository,
    article_service: ArticleService,
}

impl YuukoService {
    pub fn new(
        settings_repository: SettingsRepository,
        yuuko_state_repository: YuukoStateRepository,
        article_service: ArticleService,
    ) -> Self {
        Self {
            settings_repository,
            yuuko_state_repository,
            article_service,
        }
    }

    pub fn get_yuuko_notification_state(&self) -> Result<YuukoNotificationState, AppError> {
        let settings = self.settings_repository.load_or_default()?;
        let mut yuuko_state = self.yuuko_state_repository.load_or_default()?;
        let mut response = yuuko_state.to_notification_state();

        if !settings.notification.enabled {
            response.state = YuukoResidentState::Suppressed;
            response.has_notification = false;
            response.balloon_text = Some("通知設定がOFFになっているよ。".to_string());
            response.preview_article = None;
            response.current_article_id = None;
            response.reward_notification = None;

            // The persisted state is not updated here because this is a read-only command.
            return Ok(response);
        }

        if response.balloon_text.is_none() {
            yuuko_state.balloon_text = Some("気になるニュースを見つけたら教えるね。".to_string());
            self.yuuko_state_repository.save(&yuuko_state)?;
            response.balloon_text = yuuko_state.balloon_text.clone();
        }

        Ok(response)
    }

    pub fn confirm_rank_up_reward(
        &self,
        params: ConfirmRankUpRewardParams,
    ) -> Result<ConfirmRankUpRewardResult, AppError> {
        let mut state = self.yuuko_state_repository.load_or_default()?;
        let result = state.confirm_rank_up_reward(&params.reward_ids)?;
        self.yuuko_state_repository.save(&state)?;
        Ok(result)
    }

    /// ゆうこの通知を閉じる。pending 報酬は保持し、再通知抑制（クールタイム）を設定する。
    pub fn dismiss_yuuko_notification(&self) -> Result<YuukoNotificationState, AppError> {
        let mut state = self.yuuko_state_repository.load_or_default()?;
        state.dismiss_notification(Utc::now());
        self.yuuko_state_repository.save(&state)?;
        Ok(state.to_notification_state())
    }

    /// 無操作タイムアウト（無視）を記録する。フロントの自動退場タイマーから呼ぶ想定
    /// （Rust側はタイマー/ポーリングを持たない）。pending 報酬は保持する。
    pub fn mark_yuuko_ignored(&self) -> Result<YuukoNotificationState, AppError> {
        let mut state = self.yuuko_state_repository.load_or_default()?;
        state.mark_ignored(Utc::now());
        self.yuuko_state_repository.save(&state)?;
        Ok(state.to_notification_state())
    }

    /// ゆうこにニュース通知を出させる。MVP抑制条件（enabled / 日次上限 / クールタイム / cooldown）と
    /// 候補選定（未紹介・未読・スコア順／お気に入り除外）を満たす場合のみ、おすすめから1件を
    /// 「紹介中」状態にする（設計書 §4.3/§5.2/§6/§12）。報酬 pending 時は誤消し防止のため何もしない。
    pub fn request_yuuko_notification(&self) -> Result<RequestYuukoNotificationResult, AppError> {
        let settings = self.settings_repository.load_or_default()?;
        let mut state = self.yuuko_state_repository.load_or_default()?;

        if !settings.notification.enabled {
            return Ok(notification_result(false, "disabled", &state));
        }

        // 報酬通知が出ている間はニュース通知で上書きしない（報酬を誤って消さない・§6.4 報酬優先）。
        if state
            .reward_notification
            .as_ref()
            .is_some_and(|reward| reward.pending)
        {
            return Ok(notification_result(false, "reward_pending", &state));
        }

        // 既にアクティブな通知（ユーザー未対応）が出ている場合は上書きしない（再起動後も潰さない）。
        if state.has_active_notification() {
            return Ok(notification_result(false, "already_active", &state));
        }

        let first_range = settings
            .notification
            .work_time_ranges
            .first()
            .cloned()
            .unwrap_or_default();

        let now = Utc::now();
        match state.can_notify(
            now,
            settings.notification.max_per_day,
            &first_range.start,
            &first_range.end,
        ) {
            NotificationGate::DailyLimitReached => {
                return Ok(notification_result(false, "daily_limit", &state));
            }
            NotificationGate::CoolingDown => {
                return Ok(notification_result(false, "cooling_down", &state));
            }
            NotificationGate::OutsideTimeRange => {
                return Ok(notification_result(false, "outside_time_range", &state));
            }
            NotificationGate::Allowed => {}
        }

        // おすすめ候補（スコア順）から未紹介・未読を優先して1件選ぶ。選定はRust側責務（§2.3）。
        let candidates = self
            .article_service
            .get_recommended_articles(GetRecommendedArticlesParams::default())?;
        let Some(article) = state.pick_introducible(&candidates) else {
            return Ok(notification_result(false, "no_candidate", &state));
        };

        state.mark_notified(now, article);
        self.yuuko_state_repository.save(&state)?;
        Ok(notification_result(true, "notified", &state))
    }

    /// ゆうこクリックの2段階遷移（最小実装）。遷移が起きた場合のみ保存する。
    pub fn handle_yuuko_clicked(&self) -> Result<YuukoNotificationState, AppError> {
        let mut state = self.yuuko_state_repository.load_or_default()?;
        if state.handle_click() {
            self.yuuko_state_repository.save(&state)?;
        }
        Ok(state.to_notification_state())
    }

    pub fn initialize_default_if_missing(&self) -> Result<(), AppError> {
        if !self.yuuko_state_repository.exists() {
            let state = self.yuuko_state_repository.load_or_default()?;
            self.yuuko_state_repository.save(&state)?;
        }
        Ok(())
    }
}

/// request_yuuko_notification の結果を組み立てる（最新状態を通知DTOへ変換）。
fn notification_result(
    notified: bool,
    reason: &str,
    state: &PersistedYuukoState,
) -> RequestYuukoNotificationResult {
    RequestYuukoNotificationResult {
        notified,
        reason: reason.to_string(),
        state: state.to_notification_state(),
    }
}
