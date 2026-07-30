//! Gemini API クライアント（要約・再説明の実AI呼び出し）。
//!
//! セキュリティ方針:
//! - 接続先は**固定**の `generativelanguage.googleapis.com`。許可リスト(AiEndpoint)で検証し、
//!   任意URL取得口は作らない（呼び出し側からURLを受け取らない）。
//! - APIキーは引数で受け取り、ヘッダ `x-goog-api-key` にのみ使用する。
//!   URL・ログ・エラーメッセージには載せない。
//! - リダイレクトは無効。送信内容は与えられたプロンプト本文のみ（最小化は呼び出し側の責務）。
//! - HTTP本文はメモリへ全量展開する前に受信バイト上限を適用する（成功 256 KiB / エラー 64 KiB）。
//!   reqwest は gzip / brotli / deflate の自動展開機能を有効化していないため、アプリへ渡る本文は
//!   圧縮解凍されない生バイト（＝受信バイト）であり、上限はその実バイト数に適用される。
//! - モデルIDは既定 `gemini-2.5-flash`。環境変数 `GEMINI_MODEL` で上書き可（Rust側のみ・安全な文字種のみ許容）。
//!   モデルはURLの**パス**に入るだけで、接続先ホストは固定（許可リスト検証）から変わらない。
//!
//! 固定の信頼済みエンドポイントのため、RSS/HTML取得のような攻撃者制御URLは存在しない。
//! よって本クライアントは許可リスト検証＋リダイレクト無効で足り、DNS再解決ガードは課さない。

use std::io::Read;
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

/// 成功(2xx)レスポンス本文をメモリへ保持・JSON解析してよい受信バイト上限（防御的上限・256 KiB）。
/// 生成結果の業務上限ではなく、巨大応答の全量読み込みを防ぐための HTTP 受信上限。
const MAX_SUCCESS_BODY_BYTES: usize = 256 * 1024;
/// HTTPエラー本文（現状は接続確認の HTTP 400 分類のみが読む）を保持・解析してよい受信バイト上限（64 KiB）。
const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;
/// 段階読み取りの1回分バッファ長。上限判定は「残り許容量+1」までに絞るため過剰蓄積しない。
const BODY_READ_CHUNK_BYTES: usize = 8 * 1024;

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
            // 非2xxはステータスのみでエラー化し、本文は読まない（従来どおり）。
            // エラー詳細取得のための新規本文読み取りは追加しない。
            return Err(AppError::Network(format!(
                "Gemini API returned HTTP status {}",
                response.status()
            )));
        }

        // 本文を全量読み込む前に受信バイト上限(256 KiB)を適用する（Content-Length 早期拒否＋実読み取り制限）。
        // 上限超過時は固定エラーになり、JSON解析へは進まない。
        let bytes = read_response_body_capped(response, MAX_SUCCESS_BODY_BYTES)?;
        let body: Value = serde_json::from_slice(&bytes).map_err(|error| {
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
                // エラー本文も全量読み込まず受信上限(64 KiB)を適用する。上限超過・読み取り/解析失敗は
                // google_status なし扱い（分類は 400+None → Internal の固定分類へ。本文は取り込まない）。
                read_response_body_capped(response, MAX_ERROR_BODY_BYTES)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                    .and_then(|body| extract_google_error_status(&body))
            } else {
                None
            };
            return classify_gemini_error_status(http_status, google_status.as_deref());
        }
        // 2xx: 応答本文を判定にのみ使う（生本文は返さない）。受信上限(256 KiB)を適用し、上限超過・
        // 読み取り/解析失敗は InvalidResponse（固定分類）。生成テキストがあれば到達＋生成OK。
        match read_response_body_capped(response, MAX_SUCCESS_BODY_BYTES)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        {
            Some(body) => classify_gemini_success_body(&body),
            None => GeminiConnectionOutcome::InvalidResponse,
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

/// 受信バイト上限超過の固定エラー（本文・断片・サイズ以外の可変情報・秘密情報を含めない）。
/// AppError::Parse を用い、新しい公開エラー契約は増やさない。generate 経由なら
/// AiProviderService::request_text の既存エラー経路で Mock フォールバックへ倒れる。
fn body_limit_error() -> AppError {
    AppError::Parse("Gemini response body exceeded the receive size limit".to_string())
}

/// 宣言された Content-Length による早期拒否判定（純粋関数・テスト対象）。
/// 宣言があり `max_bytes` を超える場合のみ true（本文を読み始める前に拒否してよい）。
/// 宣言なし(None・chunked 等)は false を返し、実読み取り制限に委ねる。
fn declared_length_exceeds(content_length: Option<u64>, max_bytes: usize) -> bool {
    matches!(content_length, Some(len) if len > max_bytes as u64)
}

/// 任意の `Read` から本文を段階的に読み、`max_bytes` を超えたら本文を保持せず固定エラーにする。
/// - 一度に「残り許容量 + 1」バイトまでしか読まないため、最大でも `max_bytes + 1` バイトしか消費しない。
/// - 上限超過を検出した時点で、超過分を蓄積用 Vec へ追加せず読み取りを止める（stream を破棄する）。
/// - 上限ちょうどは成功。上限 + 1 バイト以上は拒否。判定は文字数ではなくバイト数で行う。
///
/// テストからは小さい `max_bytes` を渡して境界を検証する（本番は定数のみを渡す）。
fn read_capped(reader: &mut impl Read, max_bytes: usize) -> Result<Vec<u8>, AppError> {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; BODY_READ_CHUNK_BYTES];
    loop {
        // 残り許容量 + 1 バイトだけ要求する（超過を1バイトで検出し、過剰読み込みしない）。
        let want = (max_bytes - buf.len() + 1).min(chunk.len());
        let read = reader.read(&mut chunk[..want]).map_err(|_| {
            // 生の I/O エラー文言も取り込まない（固定文言のみ）。
            AppError::Network("failed to read Gemini response body".to_string())
        })?;
        if read == 0 {
            break; // EOF
        }
        if buf.len() + read > max_bytes {
            // 上限超過。超過チャンクは buf へ入れず、以降は読み進めない。
            return Err(body_limit_error());
        }
        buf.extend_from_slice(&chunk[..read]);
    }
    Ok(buf)
}

/// reqwest 応答本文を受信上限付きで読む（成功=256 KiB / エラー=64 KiB を呼び出し側が渡す）。
/// Content-Length 早期拒否と実読み取り制限の二重で、本文の全量メモリ展開を防ぐ。
/// 注意: 現在の reqwest 設定は gzip / brotli / deflate の自動展開機能を有効化していないため、
/// アプリへ渡る本文は圧縮解凍されない生バイト（＝受信バイト＝ここで数えるバイト）である。
fn read_response_body_capped(
    mut response: reqwest::blocking::Response,
    max_bytes: usize,
) -> Result<Vec<u8>, AppError> {
    if declared_length_exceeds(response.content_length(), max_bytes) {
        return Err(body_limit_error());
    }
    read_capped(&mut response, max_bytes)
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

    // --- 受信サイズ上限（read_capped / declared_length_exceeds）の決定的テスト ---
    // 本番定数(256 KiB / 64 KiB)ではなく小さい上限を渡して境界を検証する。
    // read_capped は任意の Read で動くため、外部通信もローカルHTTPサーバも不要。

    /// 1回の read で最大1バイトしか返さない Reader（chunk ループの複数反復・段階的読み取りを検証する）。
    struct DripReader<'a> {
        data: &'a [u8],
        pos: usize,
    }
    impl<'a> DripReader<'a> {
        fn new(data: &'a [u8]) -> Self {
            Self { data, pos: 0 }
        }
    }
    impl Read for DripReader<'_> {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            if self.pos >= self.data.len() || out.is_empty() {
                return Ok(0);
            }
            out[0] = self.data[self.pos];
            self.pos += 1;
            Ok(1)
        }
    }

    #[test]
    fn read_capped_accepts_under_and_exactly_at_limit() {
        // 上限未満。
        let mut r = &b"abc"[..];
        assert_eq!(read_capped(&mut r, 8).unwrap(), b"abc");
        // 上限ちょうど（8バイト）は成功。
        let mut r = &b"abcdefgh"[..];
        assert_eq!(read_capped(&mut r, 8).unwrap(), b"abcdefgh");
    }

    #[test]
    fn read_capped_rejects_over_limit_by_one_byte() {
        // 上限 + 1バイト（9バイト・max=8）は拒否。
        let mut r = &b"abcdefghi"[..];
        let err = read_capped(&mut r, 8).unwrap_err();
        // エラー文言は固定で、本文断片を含まない。
        let msg = err.to_string();
        assert!(!msg.contains("abc"));
        assert!(!msg.contains("defgh"));
        assert!(!msg.contains("abcdefghi"));
        assert!(msg.contains("exceeded the receive size limit"));
    }

    #[test]
    fn read_capped_judges_by_bytes_not_chars_for_multibyte() {
        // "ああ" = 6バイト（3バイト/文字 × 2）は max=8 で成功。
        let mut r = "ああ".as_bytes();
        assert_eq!(read_capped(&mut r, 8).unwrap(), "ああ".as_bytes());
        // "あああ" = 9バイト（3文字）は max=8 で拒否（文字数(3)ではなくバイト数(9)で判定）。
        let mut r = "あああ".as_bytes();
        assert!(read_capped(&mut r, 8).is_err());
    }

    #[test]
    fn read_capped_limits_actual_read_without_content_length() {
        // Content-Length を持たない（Read だけの）経路でも実読み取り量で制限される。
        // ちょうど: 8バイトを1バイトずつ供給 → 成功。
        let mut drip = DripReader::new(b"abcdefgh");
        assert_eq!(read_capped(&mut drip, 8).unwrap(), b"abcdefgh");
        // 超過: 9バイトを1バイトずつ供給 → 拒否（複数反復で蓄積した上で上限検出）。
        let mut drip = DripReader::new(b"abcdefghi");
        assert!(read_capped(&mut drip, 8).is_err());
    }

    #[test]
    fn read_capped_empty_body_is_ok() {
        let mut r = &b""[..];
        assert_eq!(read_capped(&mut r, 8).unwrap(), b"");
    }

    #[test]
    fn declared_length_exceeds_only_when_declared_over_limit() {
        // 宣言値が上限超 → 早期拒否（true）。
        assert!(declared_length_exceeds(Some(9), 8));
        // 宣言値が上限以内 → false（実読み取りに委ねる）。
        assert!(!declared_length_exceeds(Some(8), 8));
        assert!(!declared_length_exceeds(Some(0), 8));
        // 宣言なし(chunked 等) → false（実読み取り制限に委ねる）。
        assert!(!declared_length_exceeds(None, 8));
    }

    #[test]
    fn body_limit_error_is_fixed_and_leaks_nothing() {
        // 本文・断片・APIキー・URL・ヘッダを含まない固定文言。
        let msg = body_limit_error().to_string();
        assert!(msg.contains("exceeded the receive size limit"));
        assert!(!msg.contains("http"));
        assert!(!msg.contains("api"));
        assert!(!msg.contains("candidates"));
    }

    #[test]
    fn success_and_error_limits_are_the_agreed_values() {
        // 採用値: 成功 256 KiB / エラー 64 KiB。定数以外の任意値は本番から指定できない。
        assert_eq!(MAX_SUCCESS_BODY_BYTES, 256 * 1024);
        assert_eq!(MAX_ERROR_BODY_BYTES, 64 * 1024);
    }

    #[test]
    fn under_limit_bytes_parse_as_json_like_generate_does() {
        // 上限以内の本文は read_capped 後に serde_json で解析でき、既存の生成テキスト抽出へ進める。
        let body_text = r#"{"candidates":[{"content":{"parts":[{"text":"生成された要約"}]}}]}"#;
        let mut slice = body_text.as_bytes();
        let bytes = read_capped(&mut slice, MAX_SUCCESS_BODY_BYTES).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parse_generated_text(&value).unwrap(), "生成された要約");
    }

    #[test]
    fn over_limit_returns_size_error_before_json_parse() {
        // 上限超過は「サイズ超過エラー」であり、JSON解析（Parse失敗の別文言）ではないことを確認。
        // ＝上限超過後に JSON解析へ進んでいない。
        let big = vec![b'x'; 20]; // max=8 を超える
        let mut slice = big.as_slice();
        let err = read_capped(&mut slice, 8).unwrap_err();
        assert!(err.to_string().contains("exceeded the receive size limit"));
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
