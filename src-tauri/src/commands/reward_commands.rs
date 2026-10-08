//! ランク報酬関連の Tauri command。
//!
//! - `get_reward_state`: 報酬マスタと解放・未確認状態、適用中テーマを返す（読み取り）。
//!   現ランクまでの未解放があれば Rust 側で解放して保存する（冪等）。
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
