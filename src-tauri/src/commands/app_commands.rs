//! アプリ全体の操作の Tauri command。
//!
//! - `restart_app`: アプリを再起動する。データ移行の取り込み後に、画面・常駐処理を新しいデータで
//!   始め直すために使う（データ設計書 §15.7 の `restartRequired`）。Tauri 本体の `AppHandle::request_restart`
//!   を使い、プラグインは追加しない。書き出し・取り込みの実行中は、途中で止めないよう再起動を断る。
//! - `quit_resident_app`: 画面内「常駐を終了する」から常駐ごとアプリを終了する（詳細設計書 §10.1.1）。
//!   トレイの「常駐を終了する」と同じ終了要求フラグ経由で終了し、close-to-hide に横取りされない。
//!   引数は受け取らず、終了以外の操作はできない。書き出し・取り込みの実行中は `restart_app` と同じく断る。
//! - `set_main_window_unsaved_changes`: 設定画面に未保存の変更があるかを Rust へ知らせる。true の間は、
//!   非表示待機が続いてもメインウィンドウを解放（破棄）しない（D133。未保存の入力を失わないため）。
//!   受け取るのは真偽値だけで、保存内容は受け取らない。
//! - `take_pending_main_window_article`: 解放済みのメインを「詳しく見る」で作り直したとき、新しい画面が
//!   マウント時に開くべき記事IDを1回だけ取り出す（作り直した画面はイベント送信時点で未購読のため）。
//!   どちらもメインウィンドウ専用（ゆうこ用ウィンドウの capability へは付与しない）。

use tauri::{AppHandle, State};

use crate::app_lifecycle::{self, ExitSource};
use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

/// 書き出し・取り込みの実行中は、途中で止めないよう再起動・終了を断る。
fn ensure_migration_idle(migration_running: bool) -> CommandResult<()> {
    if migration_running {
        return Err(CommandError::new(
            "MIGRATION_BUSY",
            "data migration is in progress",
        ));
    }
    Ok(())
}

#[tauri::command]
pub fn restart_app(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    ensure_migration_idle(state.data_export_service.is_migration_running())?;
    // 終了要求（RunEvent::Exit）を経てから新しいプロセスを起動する。
    // single-instance プラグインは Exit で自身のロックを解放するため、再起動後の起動は弾かれない。
    app.request_restart();
    Ok(())
}

#[tauri::command]
pub fn quit_resident_app(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    ensure_migration_idle(state.data_export_service.is_migration_running())?;
    app_lifecycle::request_exit(&app, ExitSource::InAppButton);
    Ok(())
}

#[tauri::command]
pub fn set_main_window_unsaved_changes(has_unsaved: bool) {
    app_lifecycle::set_main_window_unsaved_changes(has_unsaved);
}

#[tauri::command]
pub fn take_pending_main_window_article() -> Option<String> {
    app_lifecycle::take_pending_open_article()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_running_is_rejected_with_fixed_code() {
        let error = ensure_migration_idle(true).expect_err("移行中は断る");
        assert_eq!(error.code, "MIGRATION_BUSY");
        assert_eq!(error.message, "data migration is in progress");
        assert!(ensure_migration_idle(false).is_ok());
    }
}
