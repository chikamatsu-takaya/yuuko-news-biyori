//! AIプロバイダ接続テストの結果DTOと、固定の状態・エラー種別。
//!
//! セキュリティ方針（画面詳細設計書 SCR-003 §7.5 / §7.10 / AGENTS.md §5）:
//! - APIキー・APIキーの一部・固定プロンプト・記事本文・選択文字列・生レスポンス本文・
//!   完全な外部エラーメッセージ・外部URL・HTTPヘッダは **一切含めない**。
//! - 状態・エラーは自由文字列ではなく enum（固定値）で表現する。
//! - UIは後続実装で、種別ごとに安全な固定文言を表示できる程度の情報だけを受け取る。

use serde::Serialize;

use crate::domain::settings::AiProvider;

/// 接続確認の結果状態（固定値）。
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AiProviderConnectionStatus {
    /// 利用可能（Mockは常に、Geminiは接続確認に成功したときのみ）。
    Available,
    /// 利用不可（接続失敗・APIキー未設定など）。
    Unavailable,
    /// 未実装（OpenAI / Local）。
    NotImplemented,
}

/// 固定のエラー種別（自由文字列を返さない）。UIは種別ごとに安全な文言を表示する。
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AiProviderConnectionErrorKind {
    /// APIキー未設定。
    ApiKeyMissing,
    /// 接続・ネットワーク失敗（生エラー文は含めない）。
    Network,
    /// タイムアウト。
    Timeout,
    /// 認証失敗（401 / 403）。
    Unauthorized,
    /// レート制限（429）。
    RateLimited,
    /// 利用条件の前提不足（HTTP 400 かつ Google `error.status = FAILED_PRECONDITION`）。
    /// 例: 無料枠を利用できない地域・課金設定が必要 など。**利用者が設定を変更すれば解決できる問題**で、
    /// アプリ内部の不具合（`Internal`）とは区別する。UIは「利用条件（課金/地域）の確認」を案内できる。
    FailedPrecondition,
    /// 応答が不正（2xx だが期待形式でない等）。
    InvalidResponse,
    /// 未実装Provider（OpenAI / Local）。
    ProviderNotImplemented,
    /// 内部エラー（設定読込・クライアント生成失敗、HTTP 400 の INVALID_ARGUMENT など、アプリ側の不整合）。
    Internal,
}

/// 接続テスト結果DTO（固定情報のみ）。
///
/// - `provider`: 確認対象（保存設定から読み取った現在のProvider）。**設定読込失敗で Provider を
///   決定できない場合は `None`**（`AiProvider` enum に `Unknown` 等を足さず、接続テストDTO内だけで表す）。
/// - `checked_provider`: 実際に確認したProvider。接続テストでは自動Mockフォールバックしないため
///   `provider` と一致する（通常処理の Mock フォールバックで「Gemini成功」に見えないようにするため）。
///   Provider 未確定時は `None`。
/// - `status`: 利用可否の固定状態。
/// - `error_kind`: 失敗時の固定エラー種別（成功時 None）。設定読込失敗は `Internal`。
/// - `mock_available`: MockProviderが利用可能か。Geminiの接続確認結果と区別するために別途保持する。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiProviderConnectionTestResult {
    pub provider: Option<AiProvider>,
    pub checked_provider: Option<AiProvider>,
    pub status: AiProviderConnectionStatus,
    pub error_kind: Option<AiProviderConnectionErrorKind>,
    pub mock_available: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_without_secret_fields() {
        // DTOは固定情報のみ。APIキー・プロンプト・生レスポンス等のフィールドが無いことを構造的に確認する。
        let result = AiProviderConnectionTestResult {
            provider: Some(AiProvider::Gemini),
            checked_provider: Some(AiProvider::Gemini),
            status: AiProviderConnectionStatus::Unavailable,
            error_kind: Some(AiProviderConnectionErrorKind::Unauthorized),
            mock_available: true,
        };
        let json = serde_json::to_string(&result).expect("serialize");
        assert!(json.contains("\"provider\":\"gemini\""));
        assert!(json.contains("\"status\":\"unavailable\""));
        assert!(json.contains("\"errorKind\":\"unauthorized\""));
        assert!(json.contains("\"mockAvailable\":true"));
        // 秘密情報・本文・生エラーに相当するキーが存在しない。
        for forbidden in [
            "apiKey", "api_key", "prompt", "body", "response", "url", "header", "message",
        ] {
            assert!(
                !json.contains(forbidden),
                "DTO must not contain `{forbidden}`: {json}"
            );
        }
    }

    #[test]
    fn error_kind_is_null_when_available() {
        let result = AiProviderConnectionTestResult {
            provider: Some(AiProvider::Mock),
            checked_provider: Some(AiProvider::Mock),
            status: AiProviderConnectionStatus::Available,
            error_kind: None,
            mock_available: true,
        };
        assert_eq!(result.status, AiProviderConnectionStatus::Available);
        let json = serde_json::to_string(&result).expect("serialize");
        assert!(json.contains("\"errorKind\":null"));
    }

    #[test]
    fn provider_undetermined_serializes_as_null_with_internal_error() {
        // 設定読込失敗など Provider 未確定時: provider / checkedProvider は null、
        // status=unavailable、errorKind=internal、mockAvailable=true。生エラー・秘密情報は含めない。
        let result = AiProviderConnectionTestResult {
            provider: None,
            checked_provider: None,
            status: AiProviderConnectionStatus::Unavailable,
            error_kind: Some(AiProviderConnectionErrorKind::Internal),
            mock_available: true,
        };
        let json = serde_json::to_string(&result).expect("serialize");
        assert!(json.contains("\"provider\":null"));
        assert!(json.contains("\"checkedProvider\":null"));
        assert!(json.contains("\"status\":\"unavailable\""));
        assert!(json.contains("\"errorKind\":\"internal\""));
        assert!(json.contains("\"mockAvailable\":true"));
        for forbidden in [
            "apiKey", "api_key", "prompt", "body", "response", "url", "header", "message", "path",
        ] {
            assert!(
                !json.contains(forbidden),
                "DTO must not contain `{forbidden}`: {json}"
            );
        }
    }
}
