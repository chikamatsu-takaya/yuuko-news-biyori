use crate::domain::article::{
    ArchiveSummaryDto, ArticleDetailDto, ArticleHistoryItemDto, ArticleSummaryDto,
    FavoriteUpdateResult, GetArticleDetailParams, GetRecommendedArticlesParams,
    ListArticleHistoryParams, RestoreArchivedArticleParams, RestoreArchivedArticleResult,
    UpdateArticleFavoriteParams,
};
use crate::error::AppError;
use crate::repositories::article_repository::ArticleRepository;

#[derive(Debug, Clone)]
pub struct ArticleService {
    repository: ArticleRepository,
}

impl ArticleService {
    pub fn new(repository: ArticleRepository) -> Self {
        Self { repository }
    }

    pub fn get_recommended_articles(
        &self,
        params: GetRecommendedArticlesParams,
    ) -> Result<Vec<ArticleSummaryDto>, AppError> {
        let limit = params.normalized_limit()?;
        self.repository.list_recommended(limit)
    }

    pub fn list_article_history(
        &self,
        params: ListArticleHistoryParams,
    ) -> Result<Vec<ArticleHistoryItemDto>, AppError> {
        let limit = params.normalized_limit()?;
        let filter = params.normalized_filter();
        self.repository.list_history(filter, limit)
    }

    pub fn get_article_detail(
        &self,
        params: GetArticleDetailParams,
    ) -> Result<ArticleDetailDto, AppError> {
        let article_id = params.validated_article_id()?;
        self.repository.get_article_detail(&article_id)
    }

    pub fn update_article_favorite(
        &self,
        params: UpdateArticleFavoriteParams,
    ) -> Result<FavoriteUpdateResult, AppError> {
        let (article_id, is_favorite) = params.validated_inputs()?;
        self.repository
            .update_article_favorite(&article_id, is_favorite)
    }

    /// アーカイブ退避候補を返す（read-only）。判定の基準時刻は現在UTC。
    /// 実ZIP圧縮は後続のため、ここでは候補の列挙のみを行う。
    pub fn list_archive_candidates(&self) -> Result<Vec<ArticleHistoryItemDto>, AppError> {
        self.repository.list_archive_candidates(chrono::Utc::now())
    }

    /// 退避候補を月次ZIPへ圧縮し、archived 印を付ける（増分1・非破壊／元.mdは保持）。
    /// 判定の基準時刻は現在UTC。結果サマリーを返す。
    pub fn archive_candidates(&self) -> Result<ArchiveSummaryDto, AppError> {
        self.repository.archive_candidates(chrono::Utc::now())
    }

    /// アーカイブから指定記事だけを安全に復元する。パスの解決とZIP検証はrepository側で行う。
    pub fn restore_archived_article(
        &self,
        params: RestoreArchivedArticleParams,
    ) -> Result<RestoreArchivedArticleResult, AppError> {
        let article_id = params.validated_article_id()?;
        self.repository.restore_archived_article(&article_id)
    }
}
