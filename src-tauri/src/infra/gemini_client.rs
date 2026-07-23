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
/// 接続確認専用の固定・無害プロンプト（最小量）。ログ・DTO・React側へは返さない。
const CONNECTION_CHECK_PROMPT: &str = "「接続確認OK」とだけ日本語で短く返してください。";

/// 接続確認の固定分類（生エラー文・APIキー・本文・URLを含まない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeminiConnectionOutcome {
    /// 2xx かつ生成テキストを取り出せた（到達＋認証＋生成OK）。
    Ok,
    /// 認証失敗（401 / 403）。
    Unauthorized,
    /// レート制限（429）。
    RateLimited,
    /// タイムアウト（408 / 504 / 送信時タイムアウト）。
    Timeout,
    /// 接続・ネットワーク失敗、またはその他の非2xx。
    Network,
    /// 利用条件の前提不足（HTTP 400 かつ Google `error.status = FAILED_PRECONDITION`）。
    /// 課金設定・利用可能地域などの前提不足で、利用者が設定を変更すれば解決できる。
    FailedPrecondition,
    /// 2xx だが応答が期待形式でない。
    InvalidResponse,
    /// 設定読込・URL検証・クライアント生成、HTTP 400 の INVALID_ARGUMENT などの内部不整合。
    Internal,
}

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

    /// 接続確認専用。固定・無害な最小リクエストで到達可否を確認し、固定分類で返す。
    /// `api_key` はヘッダにのみ使用し、生レスポンス本文・完全な外部エラー文・APIキーは
    /// 呼び出し側へ返さない。通常の `generate`（要約・用語解説）とは独立で、自動フォールバックはしない。
    /// 準備経路（許可リスト・モデル・URL検証・クライアント生成）は `generate` と同一の安全経路を通す。
    pub fn check_connection(&self, api_key: &str) -> GeminiConnectionOutcome {
        let Ok(allowlist) = NetworkAllowlist::load(&self.allowlist_path) else {
            return GeminiConnectionOutcome::Internal;
        };
        let model = resolve_model();
        let endpoint = format!("{GEMINI_ENDPOINT_BASE}/{model}:generateContent");
        let Ok(url) = validate_url(&endpoint, UrlPurpose::AiEndpoint, &allowlist) else {
            return GeminiConnectionOutcome::Internal;
        };
        let Ok(client) = Client::builder()
            .redirect(Policy::none())
            .timeout(self.request_timeout)
            .build()
        else {
            return GeminiConnectionOutcome::Internal;
        };

        let response = match client
            .post(url)
            .header("x-goog-api-key", api_key)
            .json(&build_request_body(CONNECTION_CHECK_PROMPT))
            .send()
        {
            Ok(response) => response,
            // 生エラー文は取り込まない（秘密情報・本文混入を避ける）。timeout / 接続失敗のみ固定分類する。
            Err(error) => {
                return if error.is_timeout() {
                    GeminiConnectionOutcome::Timeout
                } else {
                    GeminiConnectionOutcome::Network
                };
            }
        };

        let status = response.status();
        if !status.is_success() {
            let http_status = status.as_u16();
            // HTTP 400 のみ、Google 構造化エラーの固定 `error.status`（例: FAILED_PRECONDITION /
            // INVALID_ARGUMENT）を読んで分類に使う。message・本文・生エラー文は読まない/返さない。
            let google_status = if http_status == 400 {
                response
                    .json::<Value>()
                    .ok()
                    .and_then(|body| extract_google_error_status(&body))
            } else {
                None
            };
            return classify_gemini_error_status(http_status, google_status.as_deref());
        }
        // 2xx: 応答本文を判定にのみ使う（生本文は返さない）。生成テキストがあれば到達＋生成OK。
        match response.json::<Value>() {
            Ok(body) => classify_gemini_success_body(&body),
            Err(_) => GeminiConnectionOutcome::InvalidResponse,
        }
    }
}

/// 非2xxステータスを固定分類する（純粋関数・テスト対象）。生の本文・message・ヘッダは扱わない。
/// HTTP 400 は Google 構造化エラーの固定 `error.status`（`google_status`）で細分する:
/// - `FAILED_PRECONDITION`（課金設定・利用可能地域など前提不足＝利用者が設定変更で解決可能）→ `FailedPrecondition`
/// - `INVALID_ARGUMENT`（リクエスト形式・モデルID・APIバージョンの不整合＝アプリ内部の不具合）→ `Internal`
/// - それ以外/欠落 → `Internal`（内部不整合寄り・安全側）
///
/// 404 は NOT_FOUND（モデル未存在等）で内部設定不整合寄りのため `Internal`。
/// 通常処理の生成エラー分類 `generate` は変更しない。
fn classify_gemini_error_status(
    status: u16,
    google_status: Option<&str>,
) -> GeminiConnectionOutcome {
    match status {
        401 | 403 => GeminiConnectionOutcome::Unauthorized,
        408 | 504 => GeminiConnectionOutcome::Timeout,
        429 => GeminiConnectionOutcome::RateLimited,
        400 => match google_status {
            Some("FAILED_PRECONDITION") => GeminiConnectionOutcome::FailedPrecondition,
            // INVALID_ARGUMENT・不明・欠落は内部不整合として扱う（利用者側では直せない前提）。
            _ => GeminiConnectionOutcome::Internal,
        },
        404 => GeminiConnectionOutcome::Internal,
        // 500系・その他は通信/サーバ側障害としてネットワーク扱い。
        _ => GeminiConnectionOutcome::Network,
    }
}

/// Google 構造化エラーの固定 `error.status` 文字列のみを取り出す（純粋関数・テスト対象）。
/// `error.message` や他フィールドは読まない（秘密情報・本文の混入を避ける）。分類にのみ使い、返却/ログはしない。
fn extract_google_error_status(body: &Value) -> Option<String> {
    body.get("error")
        .and_then(|error| error.get("status"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// 2xx応答の本文を固定分類する（純粋関数・テスト対象）。
/// 生成テキストを取り出せれば Ok、なければ InvalidResponse。生本文は返さない。
fn classify_gemini_success_body(body: &Value) -> GeminiConnectionOutcome {
    if parse_generated_text(body).is_ok() {
        GeminiConnectionOutcome::Ok
    } else {
        GeminiConnectionOutcome::InvalidResponse
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

    #[test]
    fn classify_gemini_error_status_maps_known_codes() {
        assert_eq!(
            classify_gemini_error_status(401, None),
            GeminiConnectionOutcome::Unauthorized
        );
        assert_eq!(
            classify_gemini_error_status(403, None),
            GeminiConnectionOutcome::Unauthorized
        );
        assert_eq!(
            classify_gemini_error_status(408, None),
            GeminiConnectionOutcome::Timeout
        );
        assert_eq!(
            classify_gemini_error_status(504, None),
            GeminiConnectionOutcome::Timeout
        );
        assert_eq!(
            classify_gemini_error_status(429, None),
            GeminiConnectionOutcome::RateLimited
        );
        // 404 は NOT_FOUND（モデル未存在等）で内部設定不整合寄りのため Internal。
        assert_eq!(
            classify_gemini_error_status(404, None),
            GeminiConnectionOutcome::Internal
        );
        // 500系・その他は Network（生本文・詳細は取り込まない）。
        assert_eq!(
            classify_gemini_error_status(500, None),
            GeminiConnectionOutcome::Network
        );
        assert_eq!(
            classify_gemini_error_status(502, None),
            GeminiConnectionOutcome::Network
        );
    }

    #[test]
    fn classify_gemini_400_splits_by_google_error_status() {
        // FAILED_PRECONDITION（課金/地域など前提不足・利用者が設定変更で解決可能）→ FailedPrecondition。
        assert_eq!(
            classify_gemini_error_status(400, Some("FAILED_PRECONDITION")),
            GeminiConnectionOutcome::FailedPrecondition
        );
        // INVALID_ARGUMENT（リクエスト/モデル/APIバージョン不整合＝アプリ内部の不具合）→ Internal。
        assert_eq!(
            classify_gemini_error_status(400, Some("INVALID_ARGUMENT")),
            GeminiConnectionOutcome::Internal
        );
        // status 不明・欠落は安全側で Internal。
        assert_eq!(
            classify_gemini_error_status(400, Some("SOMETHING_ELSE")),
            GeminiConnectionOutcome::Internal
        );
        assert_eq!(
            classify_gemini_error_status(400, None),
            GeminiConnectionOutcome::Internal
        );
    }

    #[test]
    fn extract_google_error_status_reads_only_status_field() {
        let body = json!({
            "error": {
                "code": 400,
                "message": "秘密や本文が混じりうるメッセージ",
                "status": "FAILED_PRECONDITION"
            }
        });
        assert_eq!(
            extract_google_error_status(&body).as_deref(),
            Some("FAILED_PRECONDITION")
        );
        // error/status が無ければ None（message は読まない）。
        assert_eq!(extract_google_error_status(&json!({"error": {}})), None);
        assert_eq!(extract_google_error_status(&json!({})), None);
    }

    #[test]
    fn classify_gemini_success_body_ok_when_text_present() {
        let body = json!({
            "candidates": [{ "content": { "parts": [{ "text": "接続確認OK" }] } }]
        });
        assert_eq!(
            classify_gemini_success_body(&body),
            GeminiConnectionOutcome::Ok
        );
    }

    #[test]
    fn classify_gemini_success_body_invalid_when_no_text() {
        let body = json!({ "candidates": [] });
        assert_eq!(
            classify_gemini_success_body(&body),
            GeminiConnectionOutcome::InvalidResponse
        );
    }

    /// 実APIキーでの接続確認スモーク。通常CI/`cargo test` では #[ignore] により実行しない。
    /// 実行例: GEMINI_API_KEY を設定し
    ///   `cargo test --manifest-path src-tauri/Cargo.toml gemini_live_connection -- --ignored --nocapture`
    #[test]
    #[ignore = "live: requires GEMINI_API_KEY and network; run with --ignored"]
    fn gemini_live_connection_check_is_ok() {
        let raw = std::env::var("GEMINI_API_KEY").unwrap_or_default();
        let key = raw.trim();
        assert!(
            !key.is_empty(),
            "set GEMINI_API_KEY to run this live connection check"
        );

        let dir = std::env::temp_dir().join("yuuko_news_gemini_live_connection");
        let paths = AppPaths::new(dir);
        paths.ensure_storage_dirs().expect("ensure storage dirs");
        NetworkAllowlist::initialize_default_if_missing(&paths.network_allowlist_path)
            .expect("init default allowlist");

        let client = GeminiClient::new(&paths);
        // 生レスポンスは返らず、固定分類のみ。APIキー・本文は出力しない。
        assert_eq!(client.check_connection(key), GeminiConnectionOutcome::Ok);
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
