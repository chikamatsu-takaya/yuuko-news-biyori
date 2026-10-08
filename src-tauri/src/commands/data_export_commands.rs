//! データ移行用ZIP書き出しの Tauri command（データ設計書 §15.6、判断台帳 D41 / D24）。
//!
//! - `export_migration_data`: 移行対象のデータを1つのZIPにまとめ、アプリデータ直下の `exports/` へ保存する。
//!   引数は受け取らず（保存先・ファイル名は Rust 側で決める）、戻り値はファイル名と件数だけ（フルパスは返さない）。

use tauri::State;

use crate::domain::data_export::MigrationExportResultDto;
use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

#[tauri::command]
pub async fn export_migration_data(
    state: State<'_, AppState>,
) -> CommandResult<MigrationExportResultDto> {
    let data_export_service = state.data_export_service.clone();
    tauri::async_runtime::spawn_blocking(move || data_export_service.export_migration_data())
        .await
        .map_err(|error| CommandError::join_error("export-migration-data", error))?
        .map_err(CommandError::from)
}
