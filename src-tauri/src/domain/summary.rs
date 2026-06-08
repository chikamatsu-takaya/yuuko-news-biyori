use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateArticleSummaryParams {
    pub article_id: String,
}

impl GenerateArticleSummaryParams {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedArticleSummaryDto {
    pub article_id: String,
    pub summary: String,
    pub yuuko_explanation: String,
    pub focus_points: Vec<String>,
    pub yuuko_comment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiRequest {
    pub prompt_id: String,
    pub input_text: String,
    pub context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiResponse {
    pub text: String,
    /// 実際に応答を生成したプロバイダ（"gemini" = 実AI成功 / "mock" = 未設定・他プロバイダ・失敗フォールバック）。
    pub provider: String,
}

#[cfg(test)]
mod tests {
    use super::GenerateArticleSummaryParams;

    #[test]
    fn validated_article_id_trims_whitespace() {
        let params = GenerateArticleSummaryParams {
            article_id: " article-001 ".to_string(),
        };

        assert_eq!(params.validated_article_id().unwrap(), "article-001");
    }

    #[test]
    fn validated_article_id_rejects_empty_value() {
        let params = GenerateArticleSummaryParams {
            article_id: " ".to_string(),
        };

        assert!(params.validated_article_id().is_err());
    }
}
