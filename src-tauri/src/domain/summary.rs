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
    /// 要点（何が起きたか・箇条書き）。注目ポイントとは別にAIで作る（D18）。
    pub key_points: Vec<String>,
    /// 注目ポイント（なぜ面白いか・何を学べるか）。
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

/// 記事の「要点」と「注目ポイント」をAIへ依頼するときの prompt_id（v1・判断台帳 D18）。
/// 要点（何が起きたか）と注目ポイント（なぜ面白いか・何を学べるか）は別々の項目として作らせるが、
/// ローカルLLMの負荷を抑えるため、AI 呼び出しは1回にまとめて JSON の2つの配列で受け取る。
pub const ARTICLE_POINTS_PROMPT_ID: &str = "article_points_v1";

/// 要点の件数（下限・上限）。AI へは「3つ程度」で依頼し、2〜4件を受け付ける。
pub const KEY_POINTS_MIN_ITEMS: usize = 2;
pub const KEY_POINTS_MAX_ITEMS: usize = 4;
/// 注目ポイントの件数（下限・上限）。
pub const FOCUS_POINTS_MIN_ITEMS: usize = 1;
pub const FOCUS_POINTS_MAX_ITEMS: usize = 3;
/// 要点・注目ポイント1件の文字数上限（Unicode 文字数）。1文の箇条書きなので短めにする。
/// ローカルLLMの JSON スキーマ（maxLength）と保存前の検証で同じ値を使う。
pub const POINT_ITEM_MAX_CHARS: usize = 100;

/// 要点・注目ポイントのAI出力（`ARTICLE_POINTS_PROMPT_ID`）の構造化契約（v1）。
/// AI の単一文字列出力を、この JSON として **厳格に** 解析する（未知フィールドは解析失敗）。
/// 件数・文字数・文字種の検証は summary_service 側で行う。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiArticlePoints {
    pub key_points: Vec<String>,
    pub focus_points: Vec<String>,
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
