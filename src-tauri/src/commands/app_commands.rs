//! アプリ全体の操作の Tauri command。
//!
//! - `restart_app`: アプリを再起動する。データ移行の取り込み後に、画面・常駐処理を新しいデータで
//!   始め直すために使う（データ設計書 §15.7 の `restartRequired`）。Tauri 本体の `AppHandle::request_restart`
//!   を使い、プラグインは追加しない。書き出し・取り込みの実行中は、途中で止めないよう再起動を断る。

use tauri::{AppHandle, State};

use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

#[tauri::command]
pub fn restart_app(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    if state.data_export_service.is_migration_running() {
        return Err(CommandError::new(
            "MIGRATION_BUSY",
            "data migration is in progress",
        ));
    }
    // 終了要求（RunEvent::Exit）を経てから新しいプロセスを起動する。
    // single-instance プラグインは Exit で自身のロックを解放するため、再起動後の起動は弾かれない。
    app.request_restart();
    Ok(())
}
