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

/// 記事の「要点」「注目ポイント」「タグ」をAIへ依頼するときの prompt_id（v2・判断台帳 D18 / D11）。
/// 要点（何が起きたか）と注目ポイント（なぜ面白いか・何を学べるか）は別々の項目として作らせるが、
/// ローカルLLMの負荷を抑えるため、AI 呼び出しは1回にまとめて JSON の配列で受け取る。
/// v2 では同じ呼び出しで記事のタグ（3〜5個・D11）も受け取る（タグのためだけに AI を呼ばない）。
/// タグは要点・注目ポイントとは別に検証し、タグだけが不正でも要点・要約の保存は止めない。
pub const ARTICLE_POINTS_PROMPT_ID: &str = "article_points_v2";

/// 要点の件数（下限・上限）。AI へは「3つ程度」で依頼し、2〜4件を受け付ける。
pub const KEY_POINTS_MIN_ITEMS: usize = 2;
pub const KEY_POINTS_MAX_ITEMS: usize = 4;
/// 注目ポイントの件数（下限・上限）。
pub const FOCUS_POINTS_MIN_ITEMS: usize = 1;
pub const FOCUS_POINTS_MAX_ITEMS: usize = 3;
/// 要点・注目ポイント1件の文字数上限（Unicode 文字数）。1文の箇条書きなので短めにする。
/// ローカルLLMの JSON スキーマ（maxLength）と保存前の検証で同じ値を使う。
pub const POINT_ITEM_MAX_CHARS: usize = 100;
/// 記事タグの件数（下限・上限）。データ設計書 §4.4 の front matter `tags` に保存する（D11）。
pub const ARTICLE_TAGS_MIN_ITEMS: usize = 3;
pub const ARTICLE_TAGS_MAX_ITEMS: usize = 5;
/// 記事タグ1件の文字数上限（Unicode 文字数・trim 後）。ローカルLLMの JSON スキーマと保存前の検証で共有する。
pub const ARTICLE_TAG_MAX_CHARS: usize = 20;
/// 記事タグに含めない文字。カンマ・読点などの区切り文字（タグを並べて扱う側で1件が分かれて見えるのを防ぐ）と、
/// HTML の山括弧。保存前の検証で拒否し、Mock のタグからは取り除く。
pub const ARTICLE_TAG_FORBIDDEN_CHARS: &[char] =
    &[',', '、', '，', '､', ';', '；', '|', '｜', '<', '>'];

/// 要点・注目ポイント・タグのAI出力（`ARTICLE_POINTS_PROMPT_ID`）の構造化契約（v2）。
/// Mock の出力と、summary_service で検証した後の値に使う。
/// AI の生の出力は summary_service 側で、タグの型崩れが要点の解析を巻き込まないよう別の形で解析する。
/// 件数・文字数・文字種の検証は summary_service 側で行う。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiArticlePoints {
    pub key_points: Vec<String>,
    pub focus_points: Vec<String>,
    /// 記事タグ（D11）。検証に落ちた・出力に無かった場合は空（保存時は既存のタグを残す）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
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
