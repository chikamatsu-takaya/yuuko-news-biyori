//! データ移行用ZIP取り込みの Tauri command（データ設計書 §15.7、判断台帳 D41）。
//!
//! - `list_migration_imports`: アプリデータ直下の `imports/` に置かれた取り込み候補を返す（ファイル名・大きさ・作成日時だけ。フルパスは返さない）。
//! - `import_migration_data`: 候補のファイル名を1つ受け取り、検証・自動バックアップのうえで現在のデータを置き換える。
//!   受け取るのは `imports/` 内のファイル名だけで、パスは受け付けない（Rust 側で名前規則と一覧との一致を確かめる）。

use tauri::State;

use crate::domain::data_export::{MigrationImportCandidateDto, MigrationImportResultDto};
use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

#[tauri::command]
pub async fn list_migration_imports(
    state: State<'_, AppState>,
) -> CommandResult<Vec<MigrationImportCandidateDto>> {
    let data_import_service = state.data_import_service.clone();
    tauri::async_runtime::spawn_blocking(move || data_import_service.list_migration_imports())
        .await
        .map_err(|error| CommandError::join_error("list-migration-imports", error))?
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn import_migration_data(
    state: State<'_, AppState>,
    file_name: String,
) -> CommandResult<MigrationImportResultDto> {
    let data_import_service = state.data_import_service.clone();
    tauri::async_runtime::spawn_blocking(move || {
        data_import_service.import_migration_data(&file_name)
    })
    .await
    .map_err(|error| CommandError::join_error("import-migration-data", error))?
    .map_err(CommandError::from)
}
