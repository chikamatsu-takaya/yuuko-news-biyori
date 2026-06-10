use tauri::State;

use crate::domain::dictionary::{
    DeleteDictionaryEntryParams, DictionaryEntryDto, DictionaryEntryListItemDto,
    ExplainSelectedTermParams, ListDictionaryEntriesParams, SaveDictionaryEntryParams,
    UpdateDictionaryFavoriteParams, UpdateDictionaryMemoParams,
};
use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

#[tauri::command]
pub async fn explain_selected_term(
    state: State<'_, AppState>,
    params: ExplainSelectedTermParams,
) -> CommandResult<DictionaryEntryDto> {
    let dictionary_service = state.dictionary_service.clone();
    tauri::async_runtime::spawn_blocking(move || dictionary_service.explain_selected_term(params))
        .await
        .map_err(|error| {
            CommandError::new(
                "JOIN_ERROR",
                format!("failed to join explain-selected-term task: {error}"),
            )
        })?
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
    .map_err(|error| {
        CommandError::new(
            "JOIN_ERROR",
            format!("failed to join list-dictionary-entries task: {error}"),
        )
    })?
    .map_err(CommandError::from)
}

#[tauri::command]
pub async fn save_dictionary_entry(
    state: State<'_, AppState>,
    params: SaveDictionaryEntryParams,
) -> CommandResult<DictionaryEntryDto> {
    let dictionary_service = state.dictionary_service.clone();
    tauri::async_runtime::spawn_blocking(move || dictionary_service.save_dictionary_entry(params))
        .await
        .map_err(|error| {
            CommandError::new(
                "JOIN_ERROR",
                format!("failed to join save-dictionary-entry task: {error}"),
            )
        })?
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
        .map_err(|error| {
            CommandError::new(
                "JOIN_ERROR",
                format!("failed to join update-dictionary-memo task: {error}"),
            )
        })?
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
    .map_err(|error| {
        CommandError::new(
            "JOIN_ERROR",
            format!("failed to join update-dictionary-favorite task: {error}"),
        )
    })?
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
        .map_err(|error| {
            CommandError::new(
                "JOIN_ERROR",
                format!("failed to join delete-dictionary-entry task: {error}"),
            )
        })?
        .map_err(CommandError::from)
}
