//! 友情ランク関連の Tauri command。
//!
//! - `get_friendship_state`: 現在の友情ランク状態を返す（読み取り）。
//! - `record_friendship_event`: イベントを記録しポイント加算する。
//!   上限・有効イベント検証・ランクアップ判定は Rust(サービス/ドメイン)側で強制する。

use tauri::State;

use crate::domain::friendship::{
    FriendshipStateDto, RecordFriendshipEventParams, RecordFriendshipEventResult,
};
use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

#[tauri::command]
pub async fn get_friendship_state(state: State<'_, AppState>) -> CommandResult<FriendshipStateDto> {
    let friendship_service = state.friendship_service.clone();
    tauri::async_runtime::spawn_blocking(move || friendship_service.get_friendship_state())
        .await
        .map_err(|error| {
            CommandError::new(
                "JOIN_ERROR",
                format!("failed to join get-friendship-state task: {error}"),
            )
        })?
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn record_friendship_event(
    state: State<'_, AppState>,
    params: RecordFriendshipEventParams,
) -> CommandResult<RecordFriendshipEventResult> {
    let friendship_service = state.friendship_service.clone();
    tauri::async_runtime::spawn_blocking(move || {
        friendship_service.record_friendship_event(&params.event_type)
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "JOIN_ERROR",
            format!("failed to join record-friendship-event task: {error}"),
        )
    })?
    .map_err(CommandError::from)
}
