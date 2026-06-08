use serde::{Deserialize, Serialize};

use crate::domain::article::ArticleSummaryDto;
use crate::error::AppError;

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

    /// 通知を閉じる（最小実装）。balloon / preview / current_article をクリアし Waiting に戻す。
    /// pending な reward_notification と confirmed_reward_ids は保持する
    /// （報酬確認は confirm_rank_up_reward が担当するため、ここでは消さない）。
    pub fn dismiss_notification(&mut self) {
        self.balloon_text = None;
        self.preview_article = None;
        self.current_article_id = None;
        self.state = YuukoResidentState::Waiting;
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

#[cfg(test)]
mod tests {
    use super::*;

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
        }
    }

    #[test]
    fn dismiss_clears_active_notification_but_keeps_rewards() {
        let mut state = state_with_notification();
        state.dismiss_notification();

        assert_eq!(state.state, YuukoResidentState::Waiting);
        assert!(state.balloon_text.is_none());
        assert!(state.preview_article.is_none());
        assert!(state.current_article_id.is_none());
        // 報酬は維持（confirm_rank_up_reward が担当）
        assert!(state.reward_notification.is_some());
        assert_eq!(state.confirmed_reward_ids, vec!["reward-0".to_string()]);
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
}
