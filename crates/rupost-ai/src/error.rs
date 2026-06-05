use thiserror::Error;

#[derive(Debug, Error)]
pub enum AiError {
    #[error("HTTP request failed: {0}")]
    RequestFailed(#[from] reqwest::Error),

    #[error("Failed to parse JSON: {0}")]
    ParseError(#[from] serde_json::Error),

    #[error("API error ({status}): {message}")]
    ApiError { status: u16, message: String },

    #[error("Translation failed: {0}")]
    TranslationFailed(String),
}

pub type Result<T> = std::result::Result<T, AiError>;
