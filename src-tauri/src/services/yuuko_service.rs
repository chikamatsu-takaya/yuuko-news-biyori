use crate::domain::friendship::FriendshipStateDto;
use crate::domain::yuuko::{
    ConfirmRankUpRewardParams, ConfirmRankUpRewardResult, YuukoNotificationState,
    YuukoResidentState,
};
use crate::error::AppError;
use crate::repositories::settings_repository::SettingsRepository;
use crate::repositories::yuuko_state_repository::YuukoStateRepository;

#[derive(Debug, Clone)]
pub struct YuukoService {
    settings_repository: SettingsRepository,
    yuuko_state_repository: YuukoStateRepository,
}

impl YuukoService {
    pub fn new(
        settings_repository: SettingsRepository,
        yuuko_state_repository: YuukoStateRepository,
    ) -> Self {
        Self {
            settings_repository,
            yuuko_state_repository,
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

    /// ゆうこの通知を閉じる（最小実装）。pending 報酬は保持する。
    pub fn dismiss_yuuko_notification(&self) -> Result<YuukoNotificationState, AppError> {
        let mut state = self.yuuko_state_repository.load_or_default()?;
        state.dismiss_notification();
        self.yuuko_state_repository.save(&state)?;
        Ok(state.to_notification_state())
    }

    /// ゆうこクリックの2段階遷移（最小実装）。遷移が起きた場合のみ保存する。
    pub fn handle_yuuko_clicked(&self) -> Result<YuukoNotificationState, AppError> {
        let mut state = self.yuuko_state_repository.load_or_default()?;
        if state.handle_click() {
            self.yuuko_state_repository.save(&state)?;
        }
        Ok(state.to_notification_state())
    }

    /// 友情ランク状態を返す（読み取り専用の最小実装）。
    /// 永続化・ポイント加算・ランクアップ判定は後続PRで実装するため、現状は既定値を返す。
    pub fn get_friendship_state(&self) -> Result<FriendshipStateDto, AppError> {
        Ok(FriendshipStateDto::default())
    }

    pub fn initialize_default_if_missing(&self) -> Result<(), AppError> {
        if !self.yuuko_state_repository.exists() {
            let state = self.yuuko_state_repository.load_or_default()?;
            self.yuuko_state_repository.save(&state)?;
        }
        Ok(())
    }
}
