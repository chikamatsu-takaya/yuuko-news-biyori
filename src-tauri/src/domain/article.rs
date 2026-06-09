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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleHistoryItemDto {
    pub article_id: String,
    pub title: String,
    pub source_name: String,
    pub published_at_text: String,
    pub fetched_at: String,
    pub genre: String,
    pub summary: Option<String>,
    pub is_favorite: bool,
    pub read_state: ArticleReadState,
    pub is_archived: bool,
    pub recommendation_score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArticleDetailDto {
    pub article_id: String,
    pub title: String,
    pub source_name: String,
    pub original_url: String,
    pub published_at_text: String,
    pub genre: String,
    pub summary: Option<String>,
    /// 元の本文抜粋。AI要約の種・再生成の基にする（表示は summary を優先）。
    pub excerpt: Option<String>,
    pub yuuko_explanation: Option<String>,
    pub focus_points: Vec<String>,
    pub yuuko_comment: Option<String>,
    pub is_favorite: bool,
    pub keyword_candidates: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FavoriteUpdateResult {
    pub article_id: String,
    pub is_favorite: bool,
}

/// 取得パイプライン（NewsService）が新規記事を保存する際の入力。
/// `PersistedArticleRecord` は repository 内部型のため、保存用の公開入力として用意する。
/// この型は内部Rust APIでのみ使用し、Tauri command では公開しない。
#[derive(Debug, Clone)]
pub struct FetchedArticle {
    pub article_id: String,
    pub title: String,
    pub source_name: String,
    pub original_url: String,
    /// 取得時刻（UTC・RFC3339）。
    pub fetched_at: String,
    pub published_at_text: String,
    pub genre: String,
    pub tags: Vec<String>,
    pub excerpt: Option<String>,
    pub recommendation_score: f32,
    pub read_state: ArticleReadState,
}

/// 生成済み要約を記事Markdownへ永続化する際の入力（内部Rust API・Tauri commandでは公開しない）。
/// B-4決定: 生成要約は記事Markdownへ保存し、再表示はキャッシュ／更新は明示再生成とする。
#[derive(Debug, Clone)]
pub struct ArticleSummaryUpdate {
    pub summary: String,
    pub yuuko_explanation: String,
    pub focus_points: Vec<String>,
    pub yuuko_comment: String,
    /// 実際に生成に使ったプロバイダ（"gemini" / "mock"）。
    pub ai_provider: String,
    /// 生成時刻（UTC・RFC3339）。
    pub generated_at: String,
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

#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArticleHistoryFilter {
    #[default]
    All,
    Unread,
    Read,
    Favorite,
    Archived,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListArticleHistoryParams {
    pub limit: Option<u32>,
    pub filter: Option<ArticleHistoryFilter>,
}

impl ListArticleHistoryParams {
    pub fn normalized_limit(&self) -> Result<usize, AppError> {
        let limit = self.limit.unwrap_or(100);
        if limit == 0 || limit > 200 {
            return Err(AppError::Validation(
                "limit must be between 1 and 200".to_string(),
            ));
        }
        Ok(limit as usize)
    }

    pub fn normalized_filter(&self) -> ArticleHistoryFilter {
        self.filter.clone().unwrap_or_default()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetArticleDetailParams {
    pub article_id: String,
}

impl GetArticleDetailParams {
    pub fn validated_article_id(&self) -> Result<String, AppError> {
        let article_id = self.article_id.trim();
        if article_id.is_empty() {
            return Err(AppError::Validation(
                "articleId must not be empty".to_string(),
            ));
        }

        Ok(article_id.to_string())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateArticleFavoriteParams {
    pub article_id: String,
    pub is_favorite: bool,
}

impl UpdateArticleFavoriteParams {
    pub fn validated_inputs(&self) -> Result<(String, bool), AppError> {
        let article_id = self.article_id.trim();
        if article_id.is_empty() {
            return Err(AppError::Validation(
                "articleId must not be empty".to_string(),
            ));
        }

        Ok((article_id.to_string(), self.is_favorite))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ArticleHistoryFilter, GetArticleDetailParams, GetRecommendedArticlesParams,
        ListArticleHistoryParams, UpdateArticleFavoriteParams,
    };

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

    #[test]
    fn history_limit_defaults_to_one_hundred() {
        let params = ListArticleHistoryParams::default();
        assert_eq!(params.normalized_limit().unwrap(), 100);
    }

    #[test]
    fn history_limit_rejects_out_of_range_values() {
        let zero = ListArticleHistoryParams {
            limit: Some(0),
            filter: None,
        };
        assert!(zero.normalized_limit().is_err());

        let too_large = ListArticleHistoryParams {
            limit: Some(201),
            filter: None,
        };
        assert!(too_large.normalized_limit().is_err());
    }

    #[test]
    fn history_filter_defaults_to_all() {
        let params = ListArticleHistoryParams {
            limit: None,
            filter: None,
        };

        assert_eq!(params.normalized_filter(), ArticleHistoryFilter::All);
    }

    #[test]
    fn validated_article_id_trims_whitespace() {
        let params = GetArticleDetailParams {
            article_id: "  article-001  ".to_string(),
        };

        assert_eq!(params.validated_article_id().unwrap(), "article-001");
    }

    #[test]
    fn validated_article_id_rejects_empty_value() {
        let params = GetArticleDetailParams {
            article_id: "   ".to_string(),
        };

        assert!(params.validated_article_id().is_err());
    }

    #[test]
    fn update_article_favorite_params_trim_values() {
        let params = UpdateArticleFavoriteParams {
            article_id: " article-001 ".to_string(),
            is_favorite: true,
        };

        let (article_id, is_favorite) = params.validated_inputs().unwrap();
        assert_eq!(article_id, "article-001");
        assert!(is_favorite);
    }

    #[test]
    fn update_article_favorite_params_reject_empty_value() {
        let params = UpdateArticleFavoriteParams {
            article_id: " ".to_string(),
            is_favorite: false,
        };

        assert!(params.validated_inputs().is_err());
    }
}
