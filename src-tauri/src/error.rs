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

impl From<AppError> for CommandError {
    fn from(value: AppError) -> Self {
        Self::new(value.code(), value.to_string())
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
