use tauri::{AppHandle, State, WebviewWindow};

use crate::domain::yuuko::{
    ConfirmRankUpRewardParams, ConfirmRankUpRewardResult, RequestYuukoNotificationResult,
    YuukoNotificationState,
};
use crate::error::{CommandError, CommandResult};
use crate::state::AppState;
use crate::yuuko_desktop_notifier::{self, YuukoAction};

#[tauri::command]
pub async fn get_yuuko_notification_state(
    state: State<'_, AppState>,
) -> CommandResult<YuukoNotificationState> {
    let yuuko_service = state.yuuko_service.clone();
    tauri::async_runtime::spawn_blocking(move || yuuko_service.get_yuuko_notification_state())
        .await
        .map_err(|error| CommandError::join_error("yuuko-notification-state", error))?
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
        .map_err(|error| CommandError::join_error("confirm-rank-up-reward", error))?
        .map_err(CommandError::from)
}

/// ゆうこ通知を閉じる（クールタイム設定）。通知が終わったらゆうこ用ウィンドウも Rust 側で隠す。
#[tauri::command]
pub async fn dismiss_yuuko_notification(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> CommandResult<YuukoNotificationState> {
    let yuuko_service = state.yuuko_service.clone();
    // join 失敗でも後処理（ゆうこ用ウィンドウを隠す）を通すため、`?` で早期 return しない。
    let result =
        tauri::async_runtime::spawn_blocking(move || yuuko_service.dismiss_yuuko_notification())
            .await
            .map_err(|error| CommandError::join_error("dismiss-yuuko-notification", error))
            .and_then(|result| result.map_err(CommandError::from));
    yuuko_desktop_notifier::after_yuuko_action(&app, window.label(), YuukoAction::Dismiss, &result);
    result
}

/// 2段階クリックを進める。ゆうこ用ウィンドウからの場合は、段階に合わせたウィンドウの大きさ変更と、
/// 「詳しく見る」確定時のメイン前面表示・記事を開く要求を Rust 側で行う（ページにウィンドウ権限が無いため）。
#[tauri::command]
pub async fn handle_yuuko_clicked(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> CommandResult<YuukoNotificationState> {
    let yuuko_service = state.yuuko_service.clone();
    let result = tauri::async_runtime::spawn_blocking(move || yuuko_service.handle_yuuko_clicked())
        .await
        .map_err(|error| CommandError::join_error("handle-yuuko-clicked", error))
        .and_then(|result| result.map_err(CommandError::from));
    yuuko_desktop_notifier::after_yuuko_action(&app, window.label(), YuukoAction::Click, &result);
    result
}

/// ゆうこにニュース通知を出させる（抑制条件・候補選定はRust側）。結果に notified/reason/state を返す。
#[tauri::command]
pub async fn request_yuuko_notification(
    state: State<'_, AppState>,
) -> CommandResult<RequestYuukoNotificationResult> {
    let yuuko_service = state.yuuko_service.clone();
    tauri::async_runtime::spawn_blocking(move || yuuko_service.request_yuuko_notification())
        .await
        .map_err(|error| CommandError::join_error("request-yuuko-notification", error))?
        .map_err(CommandError::from)
}

/// 無操作タイムアウト（無視）を記録する。フロントの自動退場タイマーから呼ぶ。
/// 通知が終わったらゆうこ用ウィンドウも Rust 側で隠す。
#[tauri::command]
pub async fn mark_yuuko_ignored(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> CommandResult<YuukoNotificationState> {
    let yuuko_service = state.yuuko_service.clone();
    let result = tauri::async_runtime::spawn_blocking(move || yuuko_service.mark_yuuko_ignored())
        .await
        .map_err(|error| CommandError::join_error("mark-yuuko-ignored", error))
        .and_then(|result| result.map_err(CommandError::from));
    yuuko_desktop_notifier::after_yuuko_action(&app, window.label(), YuukoAction::Ignore, &result);
    result
}
