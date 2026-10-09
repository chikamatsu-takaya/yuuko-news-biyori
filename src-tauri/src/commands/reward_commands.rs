//! ランク報酬関連の Tauri command。
//!
//! - `get_reward_state`: 報酬マスタと解放・未確認状態、適用中テーマを返す（読み取り）。
//!   現ランクまでの未解放があれば Rust 側で解放して保存する（冪等）。
//! - `set_active_theme`: カスタマイズ画面のテーマ切り替え。既定・解放済みの報酬テーマ・所持済みの
//!   ガチャテーマだけを受け付け、設定 `ui.themeId` に保存する（未解放の判定を React に任せないため専用にする）。
//!
//! 確認済みにする操作は既存の `confirm_rank_up_reward`（yuuko_commands）に一本化している（D35）。

use tauri::State;

use crate::domain::reward::RewardStateDto;
use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

#[tauri::command]
pub async fn get_reward_state(state: State<'_, AppState>) -> CommandResult<RewardStateDto> {
    let reward_service = state.reward_service.clone();
    tauri::async_runtime::spawn_blocking(move || reward_service.get_reward_state())
        .await
        .map_err(|error| CommandError::join_error("get-reward-state", error))?
        .map_err(CommandError::from)
}

/// 適用中テーマを切り替える。受け取るのはテーマ ID だけで、選べないテーマは検証エラー（何も保存しない）。
/// 戻り値は更新後の報酬状態（`activeThemeId` を画面へそのまま適用する）。
#[tauri::command]
pub async fn set_active_theme(
    state: State<'_, AppState>,
    params: SetActiveThemeParams,
) -> CommandResult<RewardStateDto> {
    let reward_service = state.reward_service.clone();
    let settings_service = state.settings_service.clone();
    tauri::async_runtime::spawn_blocking(move || {
        reward_service.set_active_theme(&params.theme_id, &settings_service)
    })
    .await
    .map_err(|error| CommandError::join_error("set-active-theme", error))?
    .map_err(CommandError::from)
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetActiveThemeParams {
    pub theme_id: String,
}
