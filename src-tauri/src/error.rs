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

    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error("json parse error: {0}")]
    Json(#[from] serde_json::Error),
}

impl AppError {
    fn code(&self) -> &'static str {
        match self {
            Self::Validation(_) => "VALIDATION_ERROR",
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
