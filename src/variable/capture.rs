use crate::Result;
use crate::error::RupostError;
use reqwest::header::HeaderMap;
use serde_json::Value;
use std::collections::HashMap;

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

/// 从响应中提取变量
pub fn capture_from_response(
    response_body: &str,
    response_headers: &HeaderMap,
    captures: &[VariableCapture],
) -> Result<HashMap<String, String>> {
    let mut vars = HashMap::new();

    if captures.is_empty() {
        return Ok(vars);
    }

    // 只有当需要从 Body 提取时才解析 JSON
    let body_value: Option<Value> = if captures
        .iter()
        .any(|c| matches!(c.source, CaptureSource::Body(_)))
    {
        serde_json::from_str(response_body).ok()
    } else {
        None
    };

    for capture in captures {
        let value = match &capture.source {
            CaptureSource::Body(path) => {
                if let Some(json) = &body_value {
                    extract_from_json_path(json, path)?
                } else {
                    return Err(RupostError::ParseError(format!(
                        "Response body is not valid JSON, cannot capture '{}'",
                        capture.name
                    )));
                }
            }
            CaptureSource::Header(name) => response_headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(String::from)
                .ok_or_else(|| RupostError::Other(format!("Header '{}' not found", name)))?,
            _ => {
                return Err(RupostError::Other(format!(
                    "Unsupported capture source for '{}'",
                    capture.name
                )));
            }
        };

        vars.insert(capture.name.clone(), value);
    }

    Ok(vars)
}

/// 简单的 JSON Path 提取 (支持 . 符号)
fn extract_from_json_path(json: &Value, path: &str) -> Result<String> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = json;

    for part in parts {
        match current {
            Value::Object(map) => {
                if let Some(val) = map.get(part) {
                    current = val;
                } else {
                    return Err(RupostError::Other(format!(
                        "Key '{}' not found in path '{}'",
                        part, path
                    )));
                }
            }
            _ => {
                return Err(RupostError::Other(format!(
                    "Cannot navigate path '{}' on non-object value",
                    path
                )));
            }
        }
    }

    match current {
        Value::String(s) => Ok(s.clone()),
        Value::Number(n) => Ok(n.to_string()),
        Value::Bool(b) => Ok(b.to_string()),
        Value::Null => Ok("null".to_string()),
        _ => Ok(current.to_string()), // Object/Array 转为 JSON 字符串
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

    #[test]
    fn test_capture_from_json_body() {
        let body = r#"{"user": {"id": 123, "name": "test"}, "token": "abc-123"}"#;
        let headers = HeaderMap::new();
        let captures = vec![
            VariableCapture::from_body("user_id", "user.id"),
            VariableCapture::from_body("token", "token"),
        ];

        let vars = capture_from_response(body, &headers, &captures).unwrap();
        assert_eq!(vars.get("user_id").unwrap(), "123");
        assert_eq!(vars.get("token").unwrap(), "abc-123");
    }

    #[test]
    fn test_capture_from_header() {
        let body = "{}";
        let mut headers = HeaderMap::new();
        headers.insert("X-Token", "header-token-123".parse().unwrap());

        let captures = vec![VariableCapture::from_header("auth_token", "X-Token")];

        let vars = capture_from_response(body, &headers, &captures).unwrap();
        assert_eq!(vars.get("auth_token").unwrap(), "header-token-123");
    }

    #[test]
    fn test_capture_nested_json() {
        let body = r#"{"data": {"items": {"first": "item1"}}}"#;
        let headers = HeaderMap::new();
        let captures = vec![VariableCapture::from_body("item", "data.items.first")];

        let vars = capture_from_response(body, &headers, &captures).unwrap();
        assert_eq!(vars.get("item").unwrap(), "item1");
    }

    #[test]
    fn test_capture_missing_key() {
        let body = r#"{"data": {}}"#;
        let headers = HeaderMap::new();
        let captures = vec![VariableCapture::from_body("item", "data.missing")];

        let result = capture_from_response(body, &headers, &captures);
        assert!(result.is_err());
    }
}
