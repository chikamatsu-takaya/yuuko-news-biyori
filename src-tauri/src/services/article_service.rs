use crate::domain::article::{ArticleSummaryDto, GetRecommendedArticlesParams};
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
}
