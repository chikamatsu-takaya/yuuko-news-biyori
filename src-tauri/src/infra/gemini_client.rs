//! Gemini API クライアント（要約・再説明の実AI呼び出し）。
//!
//! セキュリティ方針:
//! - 接続先は**固定**の `generativelanguage.googleapis.com`。許可リスト(AiEndpoint)で検証し、
//!   任意URL取得口は作らない（呼び出し側からURLを受け取らない）。
//! - APIキーは引数で受け取り、ヘッダ `x-goog-api-key` にのみ使用する。
//!   URL・ログ・エラーメッセージには載せない。
//! - リダイレクトは無効。送信内容は与えられたプロンプト本文のみ（最小化は呼び出し側の責務）。
//! - モデルIDは既定 `gemini-2.5-flash`。環境変数 `GEMINI_MODEL` で上書き可（Rust側のみ・安全な文字種のみ許容）。
//!   モデルはURLの**パス**に入るだけで、接続先ホストは固定（許可リスト検証）から変わらない。
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

/// 既定のGeminiモデル。`gemini-1.5-flash` は廃止済みのため、現行の price-performance モデルを既定とする。
const DEFAULT_GEMINI_MODEL: &str = "gemini-2.5-flash";
/// モデルIDを上書きする環境変数（任意）。Rust側でのみ読む。
const GEMINI_MODEL_ENV: &str = "GEMINI_MODEL";
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
        // モデルIDは既定または GEMINI_MODEL（安全な文字種のみ）。接続先ホストは固定のため変わらない。
        let model = resolve_model();
        let endpoint = format!("{GEMINI_ENDPOINT_BASE}/{model}:generateContent");
        // 接続先ホストを許可リスト(AiEndpoint)で検証する（任意URLは扱わない）。
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

/// 使用するGeminiモデルIDを決める。`GEMINI_MODEL`（任意）が安全な文字種なら採用し、
/// 未設定・不正値なら既定 `DEFAULT_GEMINI_MODEL` を使う。
/// モデルIDはURLパスに入るため、ホスト偽装やパス細工を防ぐ目的で文字種を制限する。
fn resolve_model() -> String {
    let Ok(value) = std::env::var(GEMINI_MODEL_ENV) else {
        return DEFAULT_GEMINI_MODEL.to_string();
    };
    let trimmed = value.trim();
    if is_valid_model_id(trimmed) {
        trimmed.to_string()
    } else {
        // 不正値はログに残し（モデル名は秘密ではない）、既定へフォールバックする。
        log::warn!("GEMINI_MODEL is set but is not a valid model id; using the default model");
        DEFAULT_GEMINI_MODEL.to_string()
    }
}

/// モデルIDとして許容する文字種（英数・`.`・`-`・`_`）か判定する。
/// 空文字や `/` `@` `:` 空白などを弾き、エンドポイントURLのパス以外へ影響しないことを保証する。
fn is_valid_model_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
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

    #[test]
    fn is_valid_model_id_accepts_expected_ids() {
        assert!(is_valid_model_id("gemini-2.5-flash"));
        assert!(is_valid_model_id("gemini-2.0-flash-001"));
        assert!(is_valid_model_id(DEFAULT_GEMINI_MODEL));
    }

    #[test]
    fn is_valid_model_id_rejects_unsafe_values() {
        assert!(!is_valid_model_id("")); // 空文字
        assert!(!is_valid_model_id("models/gemini-2.5-flash")); // スラッシュ不可
        assert!(!is_valid_model_id("evil@host")); // @ 不可
        assert!(!is_valid_model_id("a b")); // 空白不可
    }

    /// 実APIキーでの疎通スモーク（B-3）。通常CI/`cargo test` では #[ignore] により実行しない。
    /// 実行例: GEMINI_API_KEY を設定し
    ///   `cargo test --manifest-path src-tauri/Cargo.toml gemini_live -- --ignored --nocapture`
    /// ネットワークと実APIキーが必要。APIキーは環境変数からのみ読み、出力には出さない。
    #[test]
    #[ignore = "live: requires GEMINI_API_KEY and network; run with --ignored"]
    fn gemini_live_smoke_generates_text() {
        let raw = std::env::var("GEMINI_API_KEY").unwrap_or_default();
        let key = raw.trim();
        assert!(
            !key.is_empty(),
            "set GEMINI_API_KEY to run this live smoke test"
        );

        // 一時ディレクトリに既定の許可リスト（generativelanguage.googleapis.com を含む）を用意する。
        let dir = std::env::temp_dir().join("yuuko_news_gemini_live_smoke");
        let paths = AppPaths::new(dir);
        paths.ensure_storage_dirs().expect("ensure storage dirs");
        NetworkAllowlist::initialize_default_if_missing(&paths.network_allowlist_path)
            .expect("init default allowlist");

        let client = GeminiClient::new(&paths);
        let text = client
            .generate(key, "「接続確認OK」とだけ日本語で短く返してください。")
            .expect("Gemini live request should succeed");

        assert!(!text.trim().is_empty(), "response must not be empty");
        // 応答のみ表示（APIキーは出さない）。
        println!("--- Gemini live response ---\n{text}\n----------------------------");
    }
}
