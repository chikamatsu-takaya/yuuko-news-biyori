//! AIプロバイダ制御。要約・再説明テキストの生成を担当する。
//!
//! - provider が `Gemini` かつ環境変数 `GEMINI_API_KEY` が設定されている場合のみ実AI（GeminiClient）を呼ぶ。
//! - APIキーは **Rust側でのみ** 読み、フロントへ渡さない・ログに出さない。
//! - キー未設定／他プロバイダ／Gemini呼び出し失敗時は MockProvider へフォールバックする（安全側）。
//! - 送信内容は要約・再説明に必要な最小限（指示＋入力本文のみ）に絞る。

use crate::domain::settings::{AiProvider, ExplanationLevel};
use crate::domain::summary::{AiRequest, AiResponse};
use crate::error::AppError;
use crate::infra::gemini_client::GeminiClient;
use crate::paths::AppPaths;

const GEMINI_API_KEY_ENV: &str = "GEMINI_API_KEY";

#[derive(Debug, Clone)]
pub struct AiProviderService {
    gemini_client: GeminiClient,
}

impl AiProviderService {
    pub fn new(paths: &AppPaths) -> Self {
        Self {
            gemini_client: GeminiClient::new(paths),
        }
    }

    pub fn request_text(
        &self,
        request: AiRequest,
        provider: AiProvider,
        explanation_level: ExplanationLevel,
    ) -> Result<AiResponse, AppError> {
        if request.input_text.trim().is_empty() {
            return Err(AppError::Validation(
                "ai request input_text must not be empty".to_string(),
            ));
        }

        // Gemini かつ APIキーが設定されている場合のみ実AIを呼ぶ。
        // それ以外（キー未設定・他プロバイダ・実AI呼び出し失敗）は mock フォールバック。
        if provider == AiProvider::Gemini {
            if let Some(api_key) = resolve_gemini_api_key() {
                let prompt = build_prompt(&request, explanation_level);
                match self.gemini_client.generate(&api_key, &prompt) {
                    Ok(text) => return Ok(AiResponse { text }),
                    // 通信・解析失敗時はアプリを止めず mock へフォールバック（CLAUDE.md §10「安全側へ倒す」）。
                    // 失敗理由は調査用にログへ残す。APIキーは generate 側で URL・ログに出さない設計。
                    Err(error) => {
                        log::warn!(
                            "Gemini request failed; falling back to the mock provider: {error}"
                        );
                    }
                }
            } else {
                // キーはログに出さない。未設定の事実のみ記録する。
                log::info!("GEMINI_API_KEY is not set; falling back to the mock provider");
            }
        }

        Ok(AiResponse {
            text: self.mock_response(request, provider, explanation_level),
        })
    }

    fn mock_response(
        &self,
        request: AiRequest,
        provider: AiProvider,
        explanation_level: ExplanationLevel,
    ) -> String {
        let provider_label = match provider {
            AiProvider::Mock => "mock",
            AiProvider::Gemini => "gemini",
            AiProvider::Openai => "openai",
            AiProvider::Local => "local",
        };

        let level_label = match explanation_level {
            ExplanationLevel::Simple => "simple",
            ExplanationLevel::Normal => "normal",
            ExplanationLevel::Detailed => "detailed",
        };

        match request.prompt_id.as_str() {
            "summary_v1" => request.input_text,
            "yuuko_explanation_v1" => request.input_text,
            "yuuko_comment_v1" => request.input_text,
            _ => format!(
                "{provider_label}/{level_label}: {}",
                request.input_text.trim()
            ),
        }
    }
}

/// 環境変数から Gemini APIキーを読む（Rust側のみ）。空文字は未設定扱い。値はログに出さない。
fn resolve_gemini_api_key() -> Option<String> {
    std::env::var(GEMINI_API_KEY_ENV)
        .ok()
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
}

/// 要約・再説明に必要な最小限のプロンプトを組み立てる（送信データ最小化）。
/// 記事メタデータ等は送らず、指示＋入力本文のみとする。
fn build_prompt(request: &AiRequest, explanation_level: ExplanationLevel) -> String {
    let level = match explanation_level {
        ExplanationLevel::Simple => "やさしく簡潔に",
        ExplanationLevel::Normal => "分かりやすく",
        ExplanationLevel::Detailed => "詳しく",
    };

    let instruction = match request.prompt_id.as_str() {
        "summary_v1" => format!("次のニュースの要点を、日本語で{level}1〜2文で要約してください。"),
        "yuuko_explanation_v1" => {
            format!("次のニュースを、日本語で{level}読者にやさしく再説明してください。")
        }
        "yuuko_comment_v1" => {
            "次のニュースに対する、親しみやすい短い感想を日本語で一言書いてください。".to_string()
        }
        _ => format!("次のテキストを日本語で{level}整えてください。"),
    };

    format!("{instruction}\n\n{}", request.input_text.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_prompt_includes_instruction_and_input_only() {
        let request = AiRequest {
            prompt_id: "summary_v1".to_string(),
            input_text: "  生成AIの新機能が発表された  ".to_string(),
            context: Some("送信されないはずのコンテキスト".to_string()),
        };
        let prompt = build_prompt(&request, ExplanationLevel::Normal);

        assert!(prompt.contains("要約してください"));
        assert!(prompt.contains("生成AIの新機能が発表された"));
        // context は送信プロンプトに含めない（最小化）。
        assert!(!prompt.contains("送信されないはず"));
    }

    #[test]
    fn build_prompt_varies_by_level() {
        let request = AiRequest {
            prompt_id: "yuuko_explanation_v1".to_string(),
            input_text: "本文".to_string(),
            context: None,
        };
        assert!(build_prompt(&request, ExplanationLevel::Simple).contains("やさしく簡潔に"));
        assert!(build_prompt(&request, ExplanationLevel::Detailed).contains("詳しく"));
    }
}
