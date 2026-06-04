//! ニュース取得コマンド。
//!
//! `refresh_news` は**任意URLを受け取らない**。取得対象は `news_sources.json` に
//! 保存された許可URLのみで、NewsService が許可リスト検証込みで取得する。

use tauri::State;

use crate::error::{CommandError, CommandResult};
use crate::services::news_service::RefreshNewsResult;
use crate::state::AppState;

/// 保存済みの取得元からニュースを取得・更新する。
/// 戻り値の errors はサニタイズ済み（対象URLと固定カテゴリのみ）。
#[tauri::command]
pub async fn refresh_news(state: State<'_, AppState>) -> CommandResult<RefreshNewsResult> {
    let news_service = state.news_service.clone();
    news_service.refresh().await.map_err(CommandError::from)
}
