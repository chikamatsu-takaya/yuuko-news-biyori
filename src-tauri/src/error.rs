use serde::Serialize;
use std::io;
use thiserror::Error;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

impl CommandError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

pub type CommandResult<T> = Result<T, CommandError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("validation error: {0}")]
    Validation(String),

    #[error("network error: {0}")]
    Network(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("parse error: {0}")]
    Parse(String),

    #[error("archive error: {0}")]
    Archive(String),

    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error("json parse error: {0}")]
    Json(#[from] serde_json::Error),
}

impl AppError {
    fn code(&self) -> &'static str {
        match self {
            Self::Validation(_) => "VALIDATION_ERROR",
            Self::Network(_) => "NETWORK_ERROR",
            Self::NotFound(_) => "NOT_FOUND_ERROR",
            Self::Parse(_) => "PARSE_ERROR",
            Self::Archive(_) => "ARCHIVE_ERROR",
            Self::Io(_) => "IO_ERROR",
            Self::Json(_) => "JSON_ERROR",
        }
    }
}

impl AppError {
    /// React へ返す固定文言（セキュリティ詳細設計書 §5.5 / §16.3）。
    ///
    /// Network / Io などの内部詳細には URL・OS のエラー文・記事ID・パスが入り得るため、
    /// 画面側へは種別ごとの固定文言だけを返す。Validation だけは各検証箇所で
    /// 人向けの固定的な理由を組み立てているため、従来どおり詳細をそのまま返す。
    fn public_message(&self) -> String {
        match self {
            Self::Validation(_) => self.to_string(),
            Self::Network(_) => "network request failed".to_string(),
            Self::NotFound(_) => "requested item was not found".to_string(),
            Self::Parse(_) => "failed to parse stored data".to_string(),
            Self::Archive(_) => "archive operation failed".to_string(),
            Self::Io(_) => "failed to access local data".to_string(),
            Self::Json(_) => "failed to read or write JSON data".to_string(),
        }
    }
}

impl From<AppError> for CommandError {
    fn from(value: AppError) -> Self {
        let code = value.code();
        // 詳細は調査用にログへだけ残す。URL 等を含み得るため debug より上げない。
        log::debug!("command error {code}: {value}");
        Self::new(code, value.public_message())
    }
}

/// 「元記事を開く」（open_original_article）の失敗種別。
///
/// React には固定のコードと文言だけを返し、保存済み URL・OS のエラー詳細は含めない。
/// 記事IDの不正・記事が見つからない等は既存の AppError をそのまま返す。
#[derive(Debug)]
pub enum OpenOriginalArticleError {
    Article(AppError),
    /// 保存済み URL が http/https でない・解釈できない等で、開いてよい形でない。
    UrlRejected,
    /// この OS では既定のブラウザを開けない（Windows 以外）。
    Unsupported,
    /// OS がブラウザの起動に失敗した。
    LaunchFailed,
}

impl From<AppError> for OpenOriginalArticleError {
    fn from(value: AppError) -> Self {
        Self::Article(value)
    }
}

impl From<OpenOriginalArticleError> for CommandError {
    fn from(value: OpenOriginalArticleError) -> Self {
        match value {
            OpenOriginalArticleError::Article(error) => error.into(),
            OpenOriginalArticleError::UrlRejected => Self::new(
                "ORIGINAL_URL_REJECTED",
                "saved original article URL cannot be opened",
            ),
            OpenOriginalArticleError::Unsupported => Self::new(
                "OPEN_BROWSER_UNSUPPORTED",
                "opening the default browser is not supported on this platform",
            ),
            OpenOriginalArticleError::LaunchFailed => {
                Self::new("OPEN_BROWSER_FAILED", "failed to open the default browser")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command_error(error: AppError) -> CommandError {
        CommandError::from(error)
    }

    #[test]
    fn network_error_hides_url_and_detail() {
        let error = command_error(AppError::Network(
            "request to https://example.com/secret?token=abc failed: connection refused"
                .to_string(),
        ));
        assert_eq!(error.code, "NETWORK_ERROR");
        assert_eq!(error.message, "network request failed");
        assert!(!error.message.contains("example.com"));
        assert!(!error.message.contains("connection refused"));
    }

    #[test]
    fn not_found_error_hides_identifier() {
        let error = command_error(AppError::NotFound("article article-123".to_string()));
        assert_eq!(error.code, "NOT_FOUND_ERROR");
        assert_eq!(error.message, "requested item was not found");
        assert!(!error.message.contains("article-123"));
    }

    #[test]
    fn parse_error_hides_detail() {
        let error = command_error(AppError::Parse(
            r"bad front matter in C:\data\a.md".to_string(),
        ));
        assert_eq!(error.code, "PARSE_ERROR");
        assert_eq!(error.message, "failed to parse stored data");
        assert!(!error.message.contains("a.md"));
    }

    #[test]
    fn archive_error_hides_detail() {
        let error = command_error(AppError::Archive("rename /tmp/x failed".to_string()));
        assert_eq!(error.code, "ARCHIVE_ERROR");
        assert_eq!(error.message, "archive operation failed");
        assert!(!error.message.contains("/tmp/x"));
    }

    #[test]
    fn io_error_hides_os_message() {
        let error = command_error(AppError::Io(io::Error::new(
            io::ErrorKind::PermissionDenied,
            r"Access is denied. (os error 5) C:\Users\someone",
        )));
        assert_eq!(error.code, "IO_ERROR");
        assert_eq!(error.message, "failed to access local data");
        assert!(!error.message.contains("os error"));
        assert!(!error.message.contains("someone"));
    }

    #[test]
    fn json_error_hides_parser_detail() {
        let json_error = serde_json::from_str::<serde_json::Value>("{ secret-content").unwrap_err();
        let error = command_error(AppError::Json(json_error));
        assert_eq!(error.code, "JSON_ERROR");
        assert_eq!(error.message, "failed to read or write JSON data");
        assert!(!error.message.contains("line"));
        assert!(!error.message.contains("secret-content"));
    }

    #[test]
    fn validation_error_keeps_message() {
        let error = command_error(AppError::Validation(
            "nickname must not be empty".to_string(),
        ));
        assert_eq!(error.code, "VALIDATION_ERROR");
        assert_eq!(
            error.message,
            "validation error: nickname must not be empty"
        );
    }
}
