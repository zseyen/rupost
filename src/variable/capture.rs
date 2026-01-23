/// 变量捕获来源
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CaptureSource {
    /// 从响应 Body 提取（使用 JSONPath）
    /// 示例: body.token, body.user.id
    Body(String),

    /// 从响应 Header 提取
    /// 示例: header.X-Token, header.Content-Type
    Header(String),

    // === P3 预留（现在添加，但返回未实现错误）===
    /// 自动识别 Trace ID（P3）
    /// 会从常见的 Trace header 中提取
    #[allow(dead_code)]
    TraceHeader,

    /// 从 Cookie 提取（P3）
    #[allow(dead_code)]
    Cookie(String),

    /// 使用正则表达式提取（P3）
    #[allow(dead_code)]
    Regex(String),
}

/// 变量捕获配置
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VariableCapture {
    /// 变量名称
    pub name: String,

    /// 捕获来源
    pub source: CaptureSource,
}

impl VariableCapture {
    /// 从 Body 提取变量
    pub fn from_body(name: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            source: CaptureSource::Body(path.into()),
        }
    }

    /// 从 Header 提取变量
    pub fn from_header(name: impl Into<String>, header_name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            source: CaptureSource::Header(header_name.into()),
        }
    }

    /// 解析捕获源字符串
    ///
    /// 语法:
    /// - `body.token` → CaptureSource::Body("token")
    /// - `body.user.id` → CaptureSource::Body("user.id")
    /// - `header.X-Token` → CaptureSource::Header("X-Token")
    pub fn parse(var_name: &str, source_str: &str) -> Self {
        let source = if let Some(path) = source_str.strip_prefix("body.") {
            CaptureSource::Body(path.to_string())
        } else if let Some(header_name) = source_str.strip_prefix("header.") {
            CaptureSource::Header(header_name.to_string())
        } else {
            // 默认从 body 提取（向后兼容）
            CaptureSource::Body(source_str.to_string())
        };

        Self {
            name: var_name.to_string(),
            source,
        }
    }

    // P3 预留
    #[allow(dead_code)]
    pub fn from_trace_header(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            source: CaptureSource::TraceHeader,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_body() {
        let capture = VariableCapture::from_body("token", "user.token");
        assert_eq!(capture.name, "token");
        assert_eq!(
            capture.source,
            CaptureSource::Body("user.token".to_string())
        );
    }

    #[test]
    fn test_from_header() {
        let capture = VariableCapture::from_header("trace_id", "X-Trace-Id");
        assert_eq!(capture.name, "trace_id");
        assert_eq!(
            capture.source,
            CaptureSource::Header("X-Trace-Id".to_string())
        );
    }

    #[test]
    fn test_parse_body() {
        let capture = VariableCapture::parse("token", "body.token");
        assert_eq!(capture.source, CaptureSource::Body("token".to_string()));

        let capture = VariableCapture::parse("user_id", "body.user.id");
        assert_eq!(capture.source, CaptureSource::Body("user.id".to_string()));
    }

    #[test]
    fn test_parse_header() {
        let capture = VariableCapture::parse("trace_id", "header.X-Trace-Id");
        assert_eq!(
            capture.source,
            CaptureSource::Header("X-Trace-Id".to_string())
        );
    }

    #[test]
    fn test_parse_default_to_body() {
        let capture = VariableCapture::parse("token", "token");
        assert_eq!(capture.source, CaptureSource::Body("token".to_string()));
    }
}
