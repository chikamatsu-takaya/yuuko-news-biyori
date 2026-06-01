use tauri::State;

use crate::domain::settings::UserSettingsDto;
use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

#[tauri::command]
pub async fn get_user_settings(state: State<'_, AppState>) -> CommandResult<UserSettingsDto> {
    let settings_service = state.settings_service.clone();
    tauri::async_runtime::spawn_blocking(move || settings_service.get_user_settings())
        .await
        .map_err(|error| {
            CommandError::new(
                "JOIN_ERROR",
                format!("failed to join settings task: {error}"),
            )
        })?
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn save_user_settings(
    state: State<'_, AppState>,
    params: SaveUserSettingsParams,
) -> CommandResult<CommandOk> {
    let settings_service = state.settings_service.clone();
    tauri::async_runtime::spawn_blocking(move || {
        settings_service.save_user_settings(params.settings)
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "JOIN_ERROR",
            format!("failed to join settings task: {error}"),
        )
    })?
    .map_err(CommandError::from)?;

    Ok(CommandOk { ok: true })
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveUserSettingsParams {
    pub settings: UserSettingsDto,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CommandOk {
    pub ok: bool,
}
