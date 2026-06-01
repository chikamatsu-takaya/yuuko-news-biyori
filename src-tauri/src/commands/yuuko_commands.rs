use tauri::State;

use crate::domain::yuuko::{
    ConfirmRankUpRewardParams, ConfirmRankUpRewardResult, YuukoNotificationState,
};
use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

#[tauri::command]
pub async fn get_yuuko_notification_state(
    state: State<'_, AppState>,
) -> CommandResult<YuukoNotificationState> {
    let yuuko_service = state.yuuko_service.clone();
    tauri::async_runtime::spawn_blocking(move || yuuko_service.get_yuuko_notification_state())
        .await
        .map_err(|error| {
            CommandError::new(
                "JOIN_ERROR",
                format!("failed to join yuuko-notification-state task: {error}"),
            )
        })?
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn confirm_rank_up_reward(
    state: State<'_, AppState>,
    params: ConfirmRankUpRewardParams,
) -> CommandResult<ConfirmRankUpRewardResult> {
    let yuuko_service = state.yuuko_service.clone();
    tauri::async_runtime::spawn_blocking(move || yuuko_service.confirm_rank_up_reward(params))
        .await
        .map_err(|error| {
            CommandError::new(
                "JOIN_ERROR",
                format!("failed to join confirm-rank-up-reward task: {error}"),
            )
        })?
        .map_err(CommandError::from)
}
