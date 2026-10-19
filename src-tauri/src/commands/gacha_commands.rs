//! ガチャ関連の Tauri command（詳細設計書 §6.6・データ設計書 §12）。
//!
//! - `get_gacha_state`: 所持かけら・コレクション（所持状態）・コンプリートかを返す（読み取り）。
//! - `draw_gacha_once`: 1回引く。不足・コンプリート時は何も消費せず理由を返す。
//! - `mark_gacha_items_seen`: 指定した排出対象の「NEW」を外す（§12.3 `newItemIds`）。
//!
//! いずれも任意のパスや任意の ID は受け付けない（ID はガチャマスタにあるものだけを Rust 側で検証する）。

use serde::Deserialize;
use tauri::State;

use crate::domain::gacha::{GachaDrawResultDto, GachaStateDto};
use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

#[tauri::command]
pub async fn get_gacha_state(state: State<'_, AppState>) -> CommandResult<GachaStateDto> {
    let gacha_service = state.gacha_service.clone();
    tauri::async_runtime::spawn_blocking(move || gacha_service.get_gacha_state())
        .await
        .map_err(|error| CommandError::join_error("get-gacha-state", error))?
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn draw_gacha_once(state: State<'_, AppState>) -> CommandResult<GachaDrawResultDto> {
    let gacha_service = state.gacha_service.clone();
    tauri::async_runtime::spawn_blocking(move || gacha_service.draw_once())
        .await
        .map_err(|error| CommandError::join_error("draw-gacha-once", error))?
        .map_err(CommandError::from)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkGachaItemsSeenParams {
    pub item_ids: Vec<String>,
}

#[tauri::command]
pub async fn mark_gacha_items_seen(
    state: State<'_, AppState>,
    params: MarkGachaItemsSeenParams,
) -> CommandResult<GachaStateDto> {
    let gacha_service = state.gacha_service.clone();
    tauri::async_runtime::spawn_blocking(move || gacha_service.mark_items_seen(&params.item_ids))
        .await
        .map_err(|error| CommandError::join_error("mark-gacha-items-seen", error))?
        .map_err(CommandError::from)
}
