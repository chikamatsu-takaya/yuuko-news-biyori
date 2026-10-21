//! データ移行用ZIP書き出しの Tauri command（データ設計書 §15.6、判断台帳 D41 / D24）。
//!
//! - `export_migration_data`: 移行対象のデータを1つのZIPにまとめ、アプリデータ直下の `exports/` へ保存する。
//!   引数は受け取らず（保存先・ファイル名は Rust 側で決める）、戻り値はファイル名と件数だけ（フルパスは返さない）。
//! - `open_migration_folder`: 書き出し先 `exports/` か取り込み元 `imports/` をエクスプローラーで開く（Windows のみ）。
//!   受け取るのは種類（`exports` / `imports`）だけで、パスは受け取らず返さない。失敗は固定のコードで返す。
//! - `get_storage_usage`: 設定画面の「ストレージ状況」用に、保存データ（移行ZIPと同じ許可リスト）の大きさを
//!   区分ごとの合計だけで返す。引数は受け取らず、パス・ファイル名は返さない。

use tauri::State;

use crate::domain::data_export::{MigrationExportResultDto, MigrationFolderKind, StorageUsageDto};
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

#[tauri::command]
pub async fn open_migration_folder(
    state: State<'_, AppState>,
    kind: MigrationFolderKind,
) -> CommandResult<()> {
    let data_export_service = state.data_export_service.clone();
    tauri::async_runtime::spawn_blocking(move || data_export_service.open_migration_folder(kind))
        .await
        .map_err(|error| CommandError::join_error("open-migration-folder", error))?
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn get_storage_usage(state: State<'_, AppState>) -> CommandResult<StorageUsageDto> {
    let data_export_service = state.data_export_service.clone();
    tauri::async_runtime::spawn_blocking(move || data_export_service.storage_usage())
        .await
        .map_err(|error| CommandError::join_error("get-storage-usage", error))?
        .map_err(CommandError::from)
}
