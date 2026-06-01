use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArticleReadState {
    Unread,
    Previewed,
    DetailViewed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleSummaryDto {
    pub article_id: String,
    pub title: String,
    pub source_name: String,
    pub published_at_text: String,
    pub genre: String,
    pub summary: Option<String>,
    pub is_favorite: bool,
    pub read_state: ArticleReadState,
    pub recommendation_score: f32,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GetRecommendedArticlesParams {
    pub limit: Option<u32>,
}

impl GetRecommendedArticlesParams {
    pub fn normalized_limit(&self) -> Result<usize, AppError> {
        let limit = self.limit.unwrap_or(20);
        if limit == 0 || limit > 50 {
            return Err(AppError::Validation(
                "limit must be between 1 and 50".to_string(),
            ));
        }
        Ok(limit as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::GetRecommendedArticlesParams;

    #[test]
    fn normalized_limit_defaults_to_twenty() {
        let params = GetRecommendedArticlesParams { limit: None };
        assert_eq!(params.normalized_limit().unwrap(), 20);
    }

    #[test]
    fn normalized_limit_accepts_valid_range() {
        let params = GetRecommendedArticlesParams { limit: Some(10) };
        assert_eq!(params.normalized_limit().unwrap(), 10);
    }

    #[test]
    fn normalized_limit_rejects_out_of_range_values() {
        let zero = GetRecommendedArticlesParams { limit: Some(0) };
        assert!(zero.normalized_limit().is_err());

        let too_large = GetRecommendedArticlesParams { limit: Some(51) };
        assert!(too_large.normalized_limit().is_err());
    }
}
