use tauri::State;

use crate::domain::dictionary::{
    DeleteDictionaryEntryParams, DictionaryEntryDto, DictionaryEntryListItemDto,
    ExplainSelectedTermParams, ListDictionaryEntriesParams, SaveDictionaryEntryParams,
    UpdateDictionaryFavoriteParams, UpdateDictionaryMemoParams,
};
use crate::error::{CommandError, CommandResult};
use crate::services::gacha_service::log_grant_failure;
use crate::state::AppState;

#[tauri::command]
pub async fn explain_selected_term(
    state: State<'_, AppState>,
    params: ExplainSelectedTermParams,
) -> CommandResult<DictionaryEntryDto> {
    let dictionary_service = state.dictionary_service.clone();
    let gacha_service = state.gacha_service.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let entry = dictionary_service.explain_selected_term(params)?;
        // 用語解説を表示できたら、流れ星のかけらを付与する（データ設計書 §12.4。辞書保存と合わせて
        // 1日の上限あり）。辞書サービスの処理を終えてから呼び、付与の失敗は解説の表示を止めない。
        log_grant_failure("term_explained", gacha_service.grant_for_term_action());
        Ok::<_, crate::error::AppError>(entry)
    })
    .await
    .map_err(|error| CommandError::join_error("explain-selected-term", error))?
    .map_err(CommandError::from)
}

#[tauri::command]
pub async fn list_dictionary_entries(
    state: State<'_, AppState>,
    params: Option<ListDictionaryEntriesParams>,
) -> CommandResult<Vec<DictionaryEntryListItemDto>> {
    let dictionary_service = state.dictionary_service.clone();
    let normalized_params = params.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        dictionary_service.list_dictionary_entries(normalized_params)
    })
    .await
    .map_err(|error| CommandError::join_error("list-dictionary-entries", error))?
    .map_err(CommandError::from)
}

#[tauri::command]
pub async fn save_dictionary_entry(
    state: State<'_, AppState>,
    params: SaveDictionaryEntryParams,
) -> CommandResult<DictionaryEntryDto> {
    let dictionary_service = state.dictionary_service.clone();
    let gacha_service = state.gacha_service.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let entry = dictionary_service.save_dictionary_entry(params)?;
        // 辞書へ保存できたら、流れ星のかけらを付与する（用語解説と合わせて1日の上限あり・§12.4）。
        log_grant_failure("dictionary_saved", gacha_service.grant_for_term_action());
        Ok::<_, crate::error::AppError>(entry)
    })
    .await
    .map_err(|error| CommandError::join_error("save-dictionary-entry", error))?
    .map_err(CommandError::from)
}

#[tauri::command]
pub async fn update_dictionary_memo(
    state: State<'_, AppState>,
    params: UpdateDictionaryMemoParams,
) -> CommandResult<DictionaryEntryListItemDto> {
    let dictionary_service = state.dictionary_service.clone();
    tauri::async_runtime::spawn_blocking(move || dictionary_service.update_dictionary_memo(params))
        .await
        .map_err(|error| CommandError::join_error("update-dictionary-memo", error))?
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn update_dictionary_favorite(
    state: State<'_, AppState>,
    params: UpdateDictionaryFavoriteParams,
) -> CommandResult<DictionaryEntryListItemDto> {
    let dictionary_service = state.dictionary_service.clone();
    tauri::async_runtime::spawn_blocking(move || {
        dictionary_service.update_dictionary_favorite(params)
    })
    .await
    .map_err(|error| CommandError::join_error("update-dictionary-favorite", error))?
    .map_err(CommandError::from)
}

#[tauri::command]
pub async fn delete_dictionary_entry(
    state: State<'_, AppState>,
    params: DeleteDictionaryEntryParams,
) -> CommandResult<String> {
    let dictionary_service = state.dictionary_service.clone();
    tauri::async_runtime::spawn_blocking(move || dictionary_service.delete_dictionary_entry(params))
        .await
        .map_err(|error| CommandError::join_error("delete-dictionary-entry", error))?
        .map_err(CommandError::from)
}
