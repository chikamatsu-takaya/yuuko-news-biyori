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

/// 用語解説をAIへ依頼するときの prompt_id（v1）。AiProviderService と DictionaryService で共有する。
/// 単一文字列出力の既存契約に、用語解説だけの構造化出力（JSON: short/detail）契約を1つ追加する。
pub const TERM_EXPLANATION_PROMPT_ID: &str = "term_explanation_v1";

/// 用語解説AI出力（`TERM_EXPLANATION_PROMPT_ID`）の構造化契約（v1）。
/// AI の単一文字列出力を、この JSON として **厳格に** 解析して short/detail を得る。
/// フィールドは `DictionaryEntryDto` の shortExplanation / detailExplanation に対応する。
/// `deny_unknown_fields` により short/detail 以外の未知フィールドを含む JSON は解析失敗にする。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiTermExplanation {
    pub short: String,
    pub detail: String,
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
