use tauri::State;

use crate::domain::yuuko::{
    ConfirmRankUpRewardParams, ConfirmRankUpRewardResult, RequestYuukoNotificationResult,
    YuukoNotificationState,
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

#[tauri::command]
pub async fn dismiss_yuuko_notification(
    state: State<'_, AppState>,
) -> CommandResult<YuukoNotificationState> {
    let yuuko_service = state.yuuko_service.clone();
    tauri::async_runtime::spawn_blocking(move || yuuko_service.dismiss_yuuko_notification())
        .await
        .map_err(|error| {
            CommandError::new(
                "JOIN_ERROR",
                format!("failed to join dismiss-yuuko-notification task: {error}"),
            )
        })?
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn handle_yuuko_clicked(
    state: State<'_, AppState>,
) -> CommandResult<YuukoNotificationState> {
    let yuuko_service = state.yuuko_service.clone();
    tauri::async_runtime::spawn_blocking(move || yuuko_service.handle_yuuko_clicked())
        .await
        .map_err(|error| {
            CommandError::new(
                "JOIN_ERROR",
                format!("failed to join handle-yuuko-clicked task: {error}"),
            )
        })?
        .map_err(CommandError::from)
}

/// ゆうこにニュース通知を出させる（抑制条件・候補選定はRust側）。結果に notified/reason/state を返す。
#[tauri::command]
pub async fn request_yuuko_notification(
    state: State<'_, AppState>,
) -> CommandResult<RequestYuukoNotificationResult> {
    let yuuko_service = state.yuuko_service.clone();
    tauri::async_runtime::spawn_blocking(move || yuuko_service.request_yuuko_notification())
        .await
        .map_err(|error| {
            CommandError::new(
                "JOIN_ERROR",
                format!("failed to join request-yuuko-notification task: {error}"),
            )
        })?
        .map_err(CommandError::from)
}

/// 無操作タイムアウト（無視）を記録する。フロントの自動退場タイマーから呼ぶ。
#[tauri::command]
pub async fn mark_yuuko_ignored(
    state: State<'_, AppState>,
) -> CommandResult<YuukoNotificationState> {
    let yuuko_service = state.yuuko_service.clone();
    tauri::async_runtime::spawn_blocking(move || yuuko_service.mark_yuuko_ignored())
        .await
        .map_err(|error| {
            CommandError::new(
                "JOIN_ERROR",
                format!("failed to join mark-yuuko-ignored task: {error}"),
            )
        })?
        .map_err(CommandError::from)
}
