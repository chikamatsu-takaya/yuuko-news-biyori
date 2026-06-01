use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleSummaryDto {
    pub article_id: String,
    pub title: String,
    pub source_name: String,
    pub published_at_text: String,
    pub genre: String,
    pub summary: Option<String>,
    pub is_favorite: bool,
    pub read_state: String,
    pub recommendation_score: f32,
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
