//! テーマの地に敷く模様の Tauri command（判断台帳 D152〜D154）。メイン画面のウィンドウだけに許可する。
//!
//! - `get_pattern_state`: 適用中の模様と、いま選べる模様の ID を返す（読み取り）。
//! - `set_active_pattern`: カスタマイズ画面の模様切り替え。なし・初期所持・所持済みのガチャ模様だけを
//!   受け付け、設定 `ui.patternId` に保存する（所持の判定を React に任せないため専用にする）。
//!
//! 色テーマの `get_reward_state` / `set_active_theme` に混ぜないのは、模様が友情ランク・報酬と無関係で、
//! 戻り値（報酬状態）にも合わないため。常駐ゆうこウィンドウには模様を敷かないので許可しない。

use tauri::State;

use crate::domain::pattern::PatternStateDto;
use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

#[tauri::command]
pub async fn get_pattern_state(state: State<'_, AppState>) -> CommandResult<PatternStateDto> {
    let pattern_service = state.pattern_service.clone();
    tauri::async_runtime::spawn_blocking(move || pattern_service.get_pattern_state())
        .await
        .map_err(|error| CommandError::join_error("get-pattern-state", error))
}

/// 模様を切り替える。受け取るのは模様 ID だけで、選べない模様は検証エラー（何も保存しない）。
#[tauri::command]
pub async fn set_active_pattern(
    state: State<'_, AppState>,
    params: SetActivePatternParams,
) -> CommandResult<PatternStateDto> {
    let pattern_service = state.pattern_service.clone();
    tauri::async_runtime::spawn_blocking(move || {
        pattern_service.set_active_pattern(&params.pattern_id)
    })
    .await
    .map_err(|error| CommandError::join_error("set-active-pattern", error))?
    .map_err(CommandError::from)
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetActivePatternParams {
    pub pattern_id: String,
}
