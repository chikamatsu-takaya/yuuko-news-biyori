use tauri::State;

use crate::domain::article::{
    ArticleDetailDto, ArticleHistoryItemDto, ArticleSummaryDto, FavoriteUpdateResult,
    GetArticleDetailParams, GetRecommendedArticlesParams, ListArticleHistoryParams,
    UpdateArticleFavoriteParams,
};
use crate::domain::summary::{GenerateArticleSummaryParams, GeneratedArticleSummaryDto};
use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

#[tauri::command]
pub async fn get_recommended_articles(
    state: State<'_, AppState>,
    params: Option<GetRecommendedArticlesParams>,
) -> CommandResult<Vec<ArticleSummaryDto>> {
    let article_service = state.article_service.clone();
    let normalized_params = params.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        article_service.get_recommended_articles(normalized_params)
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "JOIN_ERROR",
            format!("failed to join recommended-articles task: {error}"),
        )
    })?
    .map_err(CommandError::from)
}

#[tauri::command]
pub async fn list_article_history(
    state: State<'_, AppState>,
    params: Option<ListArticleHistoryParams>,
) -> CommandResult<Vec<ArticleHistoryItemDto>> {
    let article_service = state.article_service.clone();
    let normalized_params = params.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        article_service.list_article_history(normalized_params)
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "JOIN_ERROR",
            format!("failed to join article-history task: {error}"),
        )
    })?
    .map_err(CommandError::from)
}

#[tauri::command]
pub async fn get_article_detail(
    state: State<'_, AppState>,
    params: GetArticleDetailParams,
) -> CommandResult<ArticleDetailDto> {
    let article_service = state.article_service.clone();
    tauri::async_runtime::spawn_blocking(move || article_service.get_article_detail(params))
        .await
        .map_err(|error| {
            CommandError::new(
                "JOIN_ERROR",
                format!("failed to join article-detail task: {error}"),
            )
        })?
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn update_article_favorite(
    state: State<'_, AppState>,
    params: UpdateArticleFavoriteParams,
) -> CommandResult<FavoriteUpdateResult> {
    let article_service = state.article_service.clone();
    tauri::async_runtime::spawn_blocking(move || article_service.update_article_favorite(params))
        .await
        .map_err(|error| {
            CommandError::new(
                "JOIN_ERROR",
                format!("failed to join update-article-favorite task: {error}"),
            )
        })?
        .map_err(CommandError::from)
}

#[tauri::command]
pub async fn generate_article_summary(
    state: State<'_, AppState>,
    params: GenerateArticleSummaryParams,
) -> CommandResult<GeneratedArticleSummaryDto> {
    let summary_service = state.summary_service.clone();
    tauri::async_runtime::spawn_blocking(move || summary_service.generate_article_summary(params))
        .await
        .map_err(|error| {
            CommandError::new(
                "JOIN_ERROR",
                format!("failed to join generate-article-summary task: {error}"),
            )
        })?
        .map_err(CommandError::from)
}

/// アーカイブ退避候補（取得から約1か月超・非お気に入り・非archived）を返す read-only command。
/// 選定・判定はRust側。実ZIP圧縮とUI配線は後続。
#[tauri::command]
pub async fn get_archive_candidates(
    state: State<'_, AppState>,
) -> CommandResult<Vec<ArticleHistoryItemDto>> {
    let article_service = state.article_service.clone();
    tauri::async_runtime::spawn_blocking(move || article_service.list_archive_candidates())
        .await
        .map_err(|error| {
            CommandError::new(
                "JOIN_ERROR",
                format!("failed to join archive-candidates task: {error}"),
            )
        })?
        .map_err(CommandError::from)
}
