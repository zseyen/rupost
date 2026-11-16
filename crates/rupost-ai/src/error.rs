use thiserror::Error;

#[derive(Debug, Error)]
pub enum AiError {
    #[error("HTTP request failed: {0}")]
    RequestFailed(#[from] reqwest::Error),

    #[error("HTTP request failed: {0}")]
    RequestFailedMessage(String),

    #[error("Failed to parse JSON: {0}")]
    ParseError(#[from] serde_json::Error),

    #[error("Failed to parse JSON: {0}")]
    ParseErrorMessage(String),

    #[error("API error ({status}): {message}")]
    ApiError { status: u16, message: String },

    #[error("Translation failed: {0}")]
    TranslationFailed(String),
}

pub type Result<T> = std::result::Result<T, AiError>;
