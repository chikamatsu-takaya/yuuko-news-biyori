//! Gemini API クライアント（要約・再説明の実AI呼び出し）。
//!
//! セキュリティ方針:
//! - 接続先は**固定**の `generativelanguage.googleapis.com`。許可リスト(AiEndpoint)で検証し、
//!   任意URL取得口は作らない（呼び出し側からURLを受け取らない）。
//! - APIキーは引数で受け取り、ヘッダ `x-goog-api-key` にのみ使用する。
//!   URL・ログ・エラーメッセージには載せない。
//! - リダイレクトは無効。送信内容は与えられたプロンプト本文のみ（最小化は呼び出し側の責務）。
//!
//! 固定の信頼済みエンドポイントのため、RSS/HTML取得のような攻撃者制御URLは存在しない。
//! よって本クライアントは許可リスト検証＋リダイレクト無効で足り、DNS再解決ガードは課さない。

use std::path::PathBuf;
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::redirect::Policy;
use serde_json::{json, Value};

use super::allowlist::NetworkAllowlist;
use super::url_guard::{validate_url, UrlPurpose};
use crate::error::AppError;
use crate::paths::AppPaths;

const GEMINI_MODEL: &str = "gemini-1.5-flash";
const GEMINI_ENDPOINT_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/models";
const REQUEST_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone)]
pub struct GeminiClient {
    allowlist_path: PathBuf,
    request_timeout: Duration,
}

impl GeminiClient {
    pub fn new(paths: &AppPaths) -> Self {
        Self {
            allowlist_path: paths.network_allowlist_path.clone(),
            request_timeout: Duration::from_secs(REQUEST_TIMEOUT_SECS),
        }
    }

    /// プロンプトを Gemini へ送り、生成テキストを返す。
    /// `api_key` はヘッダにのみ使用し、ログ・エラー文には出さない。
    pub fn generate(&self, api_key: &str, prompt: &str) -> Result<String, AppError> {
        let allowlist = NetworkAllowlist::load(&self.allowlist_path)?;
        let endpoint = format!("{GEMINI_ENDPOINT_BASE}/{GEMINI_MODEL}:generateContent");
        // 固定エンドポイントを許可リスト(AiEndpoint)で検証する（任意URLは扱わない）。
        let url = validate_url(&endpoint, UrlPurpose::AiEndpoint, &allowlist)?;

        let client = Client::builder()
            .redirect(Policy::none())
            .timeout(self.request_timeout)
            .build()
            .map_err(|error| {
                AppError::Network(format!("failed to build Gemini client: {error}"))
            })?;

        let response = client
            .post(url)
            .header("x-goog-api-key", api_key)
            .json(&build_request_body(prompt))
            .send()
            .map_err(|error| AppError::Network(format!("Gemini request failed: {error}")))?;

        if !response.status().is_success() {
            return Err(AppError::Network(format!(
                "Gemini API returned HTTP status {}",
                response.status()
            )));
        }

        let body: Value = response.json().map_err(|error| {
            AppError::Parse(format!("failed to parse Gemini response: {error}"))
        })?;
        parse_generated_text(&body)
    }
}

/// 送信ボディ（generateContent 形式・最小）。
fn build_request_body(prompt: &str) -> Value {
    json!({
        "contents": [{
            "parts": [{ "text": prompt }]
        }]
    })
}

/// Gemini応答から生成テキスト（最初の candidate）を取り出す。
fn parse_generated_text(body: &Value) -> Result<String, AppError> {
    body.get("candidates")
        .and_then(Value::as_array)
        .and_then(|candidates| candidates.first())
        .and_then(|candidate| candidate.get("content"))
        .and_then(|content| content.get("parts"))
        .and_then(Value::as_array)
        .and_then(|parts| parts.first())
        .and_then(|part| part.get("text"))
        .and_then(Value::as_str)
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .ok_or_else(|| {
            AppError::Parse("Gemini response did not contain generated text".to_string())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_request_body_wraps_prompt() {
        let body = build_request_body("hello");
        assert_eq!(
            body["contents"][0]["parts"][0]["text"].as_str(),
            Some("hello")
        );
    }

    #[test]
    fn parse_generated_text_extracts_first_candidate() {
        let body = json!({
            "candidates": [{
                "content": { "parts": [{ "text": "  生成された要約  " }] }
            }]
        });
        assert_eq!(parse_generated_text(&body).unwrap(), "生成された要約");
    }

    #[test]
    fn parse_generated_text_errors_when_missing() {
        let body = json!({ "candidates": [] });
        assert!(parse_generated_text(&body).is_err());
    }
}
