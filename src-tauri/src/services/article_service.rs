use crate::domain::article::{
    ArticleDetailDto, ArticleSummaryDto, GetArticleDetailParams, GetRecommendedArticlesParams,
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
        Ok(self.repository.list_recommended(limit))
    }

    pub fn get_article_detail(
        &self,
        params: GetArticleDetailParams,
    ) -> Result<ArticleDetailDto, AppError> {
        let article_id = params.validated_article_id()?;
        self.repository.get_article_detail(&article_id)
    }
}
