//! ゆうこの着せかえ（吹き出し・飾り）の Tauri command（判断台帳 D155）。メイン画面のウィンドウだけに許可する。
//!
//! - `get_dressup_state`: 適用中の吹き出し・飾りと、いま選べる ID を返す（読み取り）。
//! - `set_active_balloon` / `set_active_deco`: カスタマイズ画面の切り替え。既定・所持済みのガチャ品だけを
//!   受け付け、設定 `ui.balloonId` / `ui.decoId` に保存する（所持の判定を React に任せないため専用にする）。
//!
//! 模様の `get_pattern_state` / `set_active_pattern` に混ぜないのは、模様が画面の地の見た目で、着せかえは
//! ゆうこ本人の見た目と役割が違い、戻り値（模様の状態）にも合わないため。部位ごとに用途を限った
//! 動詞の command にし、任意の欄を書き換えられる汎用 command にはしない。
//! 常駐ゆうこウィンドウには許可しない（D110）。常駐ウィンドウへは Rust がゆうこ通知のデータに着せかえの ID を載せて渡す（D156）。

use tauri::State;

use crate::domain::dressup::DressupStateDto;
use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

#[tauri::command]
pub async fn get_dressup_state(state: State<'_, AppState>) -> CommandResult<DressupStateDto> {
    let dressup_service = state.dressup_service.clone();
    tauri::async_runtime::spawn_blocking(move || dressup_service.get_dressup_state())
        .await
        .map_err(|error| CommandError::join_error("get-dressup-state", error))
}

/// 吹き出しを切り替える。受け取るのは吹き出し ID だけで、選べないものは検証エラー（何も保存しない）。
#[tauri::command]
pub async fn set_active_balloon(
    state: State<'_, AppState>,
    params: SetActiveBalloonParams,
) -> CommandResult<DressupStateDto> {
    let dressup_service = state.dressup_service.clone();
    tauri::async_runtime::spawn_blocking(move || {
        dressup_service.set_active_balloon(&params.balloon_id)
    })
    .await
    .map_err(|error| CommandError::join_error("set-active-balloon", error))?
    .map_err(CommandError::from)
}

/// 飾りを切り替える。受け取るのは飾り ID だけで、選べないものは検証エラー（何も保存しない）。
#[tauri::command]
pub async fn set_active_deco(
    state: State<'_, AppState>,
    params: SetActiveDecoParams,
) -> CommandResult<DressupStateDto> {
    let dressup_service = state.dressup_service.clone();
    tauri::async_runtime::spawn_blocking(move || dressup_service.set_active_deco(&params.deco_id))
        .await
        .map_err(|error| CommandError::join_error("set-active-deco", error))?
        .map_err(CommandError::from)
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetActiveBalloonParams {
    pub balloon_id: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetActiveDecoParams {
    pub deco_id: String,
}
