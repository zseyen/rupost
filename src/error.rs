use thiserror::Error;

#[derive(Error, Debug)]
pub enum RupostError {
    #[error("解析错误: {0}")]
    ParseError(String),

    #[error("无效的 URL: {0}")]
    InvalidUrl(String),

    #[error("HTTP 请求失败: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("网络错误: {0}")]
    NetworkError(String),

    #[error("IO 错误: {0}")]
    IoError(#[from] std::io::Error),

    #[error("JSON 解析错误: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("URL 解析错误: {0}")]
    UrlParseError(#[from] url::ParseError),

    #[error("{0}")]
    Other(String),
}

// Add conversion from anyhow::Error
impl From<anyhow::Error> for RupostError {
    fn from(err: anyhow::Error) -> Self {
        RupostError::Other(err.to_string())
    }
}

// Add conversion from parser::ParseError
impl From<crate::parser::ParseError> for RupostError {
    fn from(err: crate::parser::ParseError) -> Self {
        RupostError::ParseError(err.to_string())
    }
}

/// Result type for rupost crate
pub type Result<T> = std::result::Result<T, RupostError>;
