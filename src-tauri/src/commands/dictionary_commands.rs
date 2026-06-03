use tauri::State;

use crate::domain::dictionary::{
    DictionaryEntryDto, ExplainSelectedTermParams, SaveDictionaryEntryParams,
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
