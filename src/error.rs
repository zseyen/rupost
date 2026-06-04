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

    #[error("使用了 base_url/baseUrl 变量，但是没有在当前环境中配置它。")]
    BaseUrlNotConfigured,

    #[error("构建请求失败: {0}")]
    RequestBuildFailed(String),

    #[error("请求执行失败: {0}")]
    RequestExecutionFailed(String),

    #[error("循环依赖错误: {0}")]
    CyclicDependency(String),

    #[error("{0}")]
    Other(String),
}

impl RupostError {
    /// 针对特定错误，提供解决该问题的操作指引
    pub fn suggestion(&self) -> Option<&'static str> {
        match self {
            RupostError::BaseUrlNotConfigured => Some(
                "请在 rupost.toml 对应的环境配置 base_url，或者在执行命令时使用 --env 选项指定环境 (如 `--env dev`)，或使用 `-v base_url=...` 传入变量。",
            ),
            _ => None,
        }
    }

    /// 生成面向终端用户的友好提示字符串（包含可选的解决建议）
    pub fn to_user_friendly_string(&self) -> String {
        if let Some(sug) = self.suggestion() {
            format!("{}\n提示: {}", self, sug)
        } else {
            self.to_string()
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_to_user_friendly_string() {
        // 测试带有 suggestion 的错误
        let err = RupostError::BaseUrlNotConfigured;
        let err_str = err.to_user_friendly_string();
        assert!(err_str.contains("使用了 base_url/baseUrl 变量，但是没有在当前环境中配置它"));
        assert!(err_str.contains("提示: 请在 rupost.toml 对应的环境配置 base_url"));

        // 测试没有 suggestion 的错误
        let err_parse = RupostError::ParseError("invalid syntax".to_string());
        let err_parse_str = err_parse.to_user_friendly_string();
        assert_eq!(err_parse_str, "解析错误: invalid syntax");
    }
}
