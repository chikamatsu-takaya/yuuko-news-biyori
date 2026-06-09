use crate::domain::article::{
    ArticleDetailDto, ArticleHistoryItemDto, ArticleSummaryDto, FavoriteUpdateResult,
    GetArticleDetailParams, GetRecommendedArticlesParams, ListArticleHistoryParams,
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
}
