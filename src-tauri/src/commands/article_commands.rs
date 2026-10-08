use tauri::State;

use crate::domain::article::{
    ArchiveMonthArticlesDto, ArchiveMonthDto, ArchiveRetirementSummaryDto, ArchiveSummaryDto,
    ArticleDetailDto, ArticleHistoryItemDto, ArticleSummaryDto, FavoriteUpdateResult,
    GetArticleDetailParams, GetRecommendedArticlesParams, ListArchiveMonthArticlesParams,
    ListArticleHistoryParams, OpenOriginalArticleParams, RestoreArchivedArticleParams,
    RestoreArchivedArticleResult, UpdateArticleFavoriteParams,
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
    let auto_summary_queue = state.auto_summary_queue.clone();
    let normalized_params = params.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        let mut articles = article_service.get_recommended_articles(normalized_params)?;
        // 自動要約の待機中・処理中・失敗はメモリ上の状態のため、ここで記事ファイル由来の値に重ねる。
        auto_summary_queue.apply_to_summaries(&mut articles);
        Ok::<_, crate::error::AppError>(articles)
    })
    .await
    .map_err(|error| CommandError::join_error("recommended-articles", error))?
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
    .map_err(|error| CommandError::join_error("article-history", error))?
    .map_err(CommandError::from)
}

#[tauri::command]
pub async fn get_article_detail(
    state: State<'_, AppState>,
    params: GetArticleDetailParams,
) -> CommandResult<ArticleDetailDto> {
    let article_service = state.article_service.clone();
    let auto_summary_queue = state.auto_summary_queue.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut detail = article_service.get_article_detail(params)?;
        auto_summary_queue.apply_to_detail(&mut detail);
        Ok::<_, crate::error::AppError>(detail)
    })
    .await
    .map_err(|error| CommandError::join_error("article-detail", error))?
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
        .map_err(|error| CommandError::join_error("update-article-favorite", error))?
        .map_err(CommandError::from)
}

/// 記事IDだけを受け取り、保存済みの元記事 URL を検証して既定のブラウザで開く（判断台帳 D13）。
/// URL・パスは受け取らないため、任意の URL を開く入口にはならない。失敗は固定のコードで返す。
#[tauri::command]
pub async fn open_original_article(
    state: State<'_, AppState>,
    params: OpenOriginalArticleParams,
) -> CommandResult<()> {
    let article_service = state.article_service.clone();
    tauri::async_runtime::spawn_blocking(move || article_service.open_original_article(params))
        .await
        .map_err(|error| CommandError::join_error("open-original-article", error))?
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
        .map_err(|error| CommandError::join_error("generate-article-summary", error))?
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
        .map_err(|error| CommandError::join_error("archive-candidates", error))?
        .map_err(CommandError::from)
}

/// 退避候補を月次ZIPへ圧縮し archived 印を付ける（増分1・非破壊）。手動トリガの read-write command。
/// 元.md削除・自動スケジューラ化は後続。
#[tauri::command]
pub async fn archive_old_articles(state: State<'_, AppState>) -> CommandResult<ArchiveSummaryDto> {
    let article_service = state.article_service.clone();
    tauri::async_runtime::spawn_blocking(move || article_service.archive_candidates())
        .await
        .map_err(|error| CommandError::join_error("archive-old-articles", error))?
        .map_err(CommandError::from)
}

/// 記事IDだけを受け取り、Rust側で対応する月次ZIPとentryを特定して単記事復元する。
#[tauri::command]
pub async fn restore_archived_article(
    state: State<'_, AppState>,
    params: RestoreArchivedArticleParams,
) -> CommandResult<RestoreArchivedArticleResult> {
    let article_service = state.article_service.clone();
    tauri::async_runtime::spawn_blocking(move || article_service.restore_archived_article(params))
        .await
        .map_err(|error| CommandError::join_error("restore-archived-article", error))?
        .map_err(CommandError::from)
}

/// 完全性を検証できたarchived Markdownだけを、ロールバック可能な退避を経て通常領域から削除する。
#[tauri::command]
pub async fn retire_archived_markdown(
    state: State<'_, AppState>,
) -> CommandResult<ArchiveRetirementSummaryDto> {
    let article_service = state.article_service.clone();
    tauri::async_runtime::spawn_blocking(move || article_service.retire_archived_markdown())
        .await
        .map_err(|error| CommandError::join_error("retire-archived-markdown", error))?
        .map_err(CommandError::from)
}

/// 過去ニュース画面の月別アーカイブ一覧（年月・件数）を新しい月から返す read-only command。
/// `archive_index.json` だけを読み、ZIPは開かない。
#[tauri::command]
pub async fn list_archive_months(
    state: State<'_, AppState>,
) -> CommandResult<Vec<ArchiveMonthDto>> {
    let article_service = state.article_service.clone();
    tauri::async_runtime::spawn_blocking(move || article_service.list_archive_months())
        .await
        .map_err(|error| CommandError::join_error("archive-months", error))?
        .map_err(CommandError::from)
}

/// 年月（`YYYY-MM`）だけを受け取り、その月のアーカイブ記事一覧を記事カタログから返す read-only command。
/// パス・ファイル名は受け取らず、ZIPも展開しない。
#[tauri::command]
pub async fn list_archive_month_articles(
    state: State<'_, AppState>,
    params: ListArchiveMonthArticlesParams,
) -> CommandResult<ArchiveMonthArticlesDto> {
    let article_service = state.article_service.clone();
    tauri::async_runtime::spawn_blocking(move || {
        article_service.list_archive_month_articles(params)
    })
    .await
    .map_err(|error| CommandError::join_error("archive-month-articles", error))?
    .map_err(CommandError::from)
}
