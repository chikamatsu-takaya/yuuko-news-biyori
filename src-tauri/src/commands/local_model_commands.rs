//! ローカルLLMのモデルの初回ダウンロードの command（設計案 §3.4）。メインウィンドウだけに許可する。
//!
//! どれも引数に URL・パス・ハッシュを取らない（取るのはコードの表の 1 行だけ）。戻り値は固定の分類とバイト数だけ。
//! 取得そのものは専用のスレッドで行うため、command はすぐ返す（`discard` だけは取得が止まるまで待つ）。

use tauri::State;

use crate::error::{AppError, CommandError, CommandResult};
use crate::services::local_model_download_service::{LocalModelDownloadService, LocalModelStatus};
use crate::state::AppState;

async fn run_blocking<T: Send + 'static>(
    service: LocalModelDownloadService,
    task: impl FnOnce(LocalModelDownloadService) -> Result<T, AppError> + Send + 'static,
) -> CommandResult<T> {
    tauri::async_runtime::spawn_blocking(move || task(service))
        .await
        .map_err(|error| CommandError::join_error("local-model", error))?
        .map_err(CommandError::from)
}

/// 取得の状態（未取得・取得中・一時停止・使える・失敗・止められている・取得元なし）を返す。
#[tauri::command]
pub async fn get_local_model_status(state: State<'_, AppState>) -> CommandResult<LocalModelStatus> {
    run_blocking(state.local_model_download_service.clone(), |service| {
        Ok(service.status())
    })
    .await
}

/// 取得を始める・再開する。`auto_started` は画面が一言ことわりを表示した後に自動で始めるとき
/// （設計案 §2.1 の不変条件。自動で始めたことがあれば始めない。記録を保存できなければ始めない）。
#[tauri::command]
pub async fn start_local_model_download(
    state: State<'_, AppState>,
    auto_started: Option<bool>,
) -> CommandResult<LocalModelStatus> {
    let auto_started = auto_started.unwrap_or(false);
    run_blocking(state.local_model_download_service.clone(), move |service| {
        service.start(auto_started)
    })
    .await
}

/// 一時停止する（途中のファイルを残す。再開は `start_local_model_download`）。
#[tauri::command]
pub async fn pause_local_model_download(
    state: State<'_, AppState>,
) -> CommandResult<LocalModelStatus> {
    run_blocking(state.local_model_download_service.clone(), |service| {
        Ok(service.pause())
    })
    .await
}

/// キャンセルする（待っている自動のやり直しも止める。途中のファイルは残し、次は続きから取る）。
/// 取得前（画面のことわりの待ちの間）でも「自動で始めた」記録を書き、次の起動から自動では始めない。
#[tauri::command]
pub async fn cancel_local_model_download(
    state: State<'_, AppState>,
) -> CommandResult<LocalModelStatus> {
    run_blocking(state.local_model_download_service.clone(), |service| {
        service.cancel()
    })
    .await
}

/// 取得したモデル・途中のファイルを消す（取得中なら止め、llama-server を止めてから消す）。
#[tauri::command]
pub async fn discard_local_model(state: State<'_, AppState>) -> CommandResult<LocalModelStatus> {
    run_blocking(state.local_model_download_service.clone(), |service| {
        service.discard()
    })
    .await
}
