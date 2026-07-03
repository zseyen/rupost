#![allow(clippy::collapsible_if)]
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

    /// 使用正则表达式提取
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
        } else if let Some(regex_pattern) = source_str.strip_prefix("regex ") {
            let mut pattern = regex_pattern.trim();
            if pattern.starts_with('"') && pattern.ends_with('"') && pattern.len() >= 2 {
                pattern = &pattern[1..pattern.len() - 1];
            }
            CaptureSource::Regex(pattern.to_string())
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

    /// 提取常规响应的变量，如果存在捕获配置，则将其存入 `VariableContext`
    pub fn capture_normal(
        captures: &[VariableCapture],
        body: &str,
        headers: &HeaderMap,
        context: &mut crate::variable::VariableContext,
    ) {
        if captures.is_empty() {
            return;
        }
        match capture_from_response(body, headers, captures) {
            Ok(captured_vars) => {
                for (key, value) in &captured_vars {
                    tracing::debug!("Captured variable: {} = '{}'", key, value);
                }
                context.extend(captured_vars);
            }
            Err(e) => {
                tracing::error!("Failed to capture variables: {}", e);
            }
        }
    }

    /// 提取 SSE 单帧事件实时捕获，并在 `VariableContext` 中生效
    pub fn capture_sse_frame(
        captures: &[VariableCapture],
        virtual_body: &str,
        virtual_headers: &HeaderMap,
        context: &mut crate::variable::VariableContext,
    ) {
        let mut stream_captures = Vec::new();
        for cap in captures {
            if let CaptureSource::Body(ref path) = cap.source {
                if let Some(rest) = path.strip_prefix("stream.body.") {
                    let mut new_cap = cap.clone();
                    new_cap.source = CaptureSource::Body(rest.to_string());
                    stream_captures.push(new_cap);
                } else if path == "stream.event" {
                    let mut new_cap = cap.clone();
                    new_cap.source = CaptureSource::Header("x-sse-event".to_string());
                    stream_captures.push(new_cap);
                } else if path == "stream.id" {
                    let mut new_cap = cap.clone();
                    new_cap.source = CaptureSource::Header("x-sse-id".to_string());
                    stream_captures.push(new_cap);
                }
            } else if let CaptureSource::Header(ref name) = cap.source {
                if name == "stream.event" {
                    let mut new_cap = cap.clone();
                    new_cap.source = CaptureSource::Header("x-sse-event".to_string());
                    stream_captures.push(new_cap);
                } else if name == "stream.id" {
                    let mut new_cap = cap.clone();
                    new_cap.source = CaptureSource::Header("x-sse-id".to_string());
                    stream_captures.push(new_cap);
                }
            }
        }

        if !stream_captures.is_empty()
            && let Ok(captured_vars) =
                capture_from_response(virtual_body, virtual_headers, &stream_captures)
        {
            context.extend(captured_vars);
        }
    }

    /// 提取流结束后针对完整 LLM 内容的捕获 (针对 `stream.llm.content`)
    pub fn capture_sse_llm_content(
        captures: &[VariableCapture],
        final_body: &str,
        final_headers: &HeaderMap,
        context: &mut crate::variable::VariableContext,
    ) {
        let mut final_captures = Vec::new();
        for cap in captures {
            if matches!(&cap.source, CaptureSource::Body(path) if path == "stream.llm.content") {
                let mut new_cap = cap.clone();
                new_cap.source = CaptureSource::Header("x-sse-llm-content".to_string());
                final_captures.push(new_cap);
            }
        }
        if !final_captures.is_empty() {
            match capture_from_response(final_body, final_headers, &final_captures) {
                Ok(captured_vars) => {
                    context.extend(captured_vars);
                }
                Err(e) => {
                    tracing::error!(
                        "Failed to capture stream variables: {:?}. headers: {:?}",
                        e,
                        final_headers
                    );
                }
            }
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
            CaptureSource::Header(name) => {
                let raw_val = response_headers
                    .get(name)
                    .map(|v| String::from_utf8_lossy(v.as_bytes()).to_string())
                    .ok_or_else(|| RupostError::Other(format!("Header '{}' not found", name)))?;
                if name == "x-sse-llm-content" {
                    url::form_urlencoded::parse(raw_val.as_bytes())
                        .map(|(key, _)| key)
                        .collect::<String>()
                } else {
                    raw_val
                }
            }
            CaptureSource::Regex(pattern) => {
                let re = regex::Regex::new(pattern).map_err(|e| {
                    RupostError::ParseError(format!("Invalid regex pattern '{}': {}", pattern, e))
                })?;

                if let Some(caps) = re.captures(response_body) {
                    // 如果有捕获组，提取第一个捕获组 (索引1)
                    // 否则提取整个匹配的内容 (索引0)
                    if let Some(matched) = caps.get(1).or_else(|| caps.get(0)) {
                        matched.as_str().to_string()
                    } else {
                        return Err(RupostError::Other(format!(
                            "Regex '{}' matched but captured nothing for '{}'",
                            pattern, capture.name
                        )));
                    }
                } else {
                    return Err(RupostError::Other(format!(
                        "Regex '{}' matched nothing in response body for '{}'",
                        pattern, capture.name
                    )));
                }
            }
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

/// 简单的 JSON Path 提取 (支持 . 和 [] 符号)
fn extract_from_json_path(json: &Value, path: &str) -> Result<String> {
    // 允许 items[0] 语法，转换为 items.0
    let normalized_path = path.replace('[', ".").replace(']', "");
    let parts: Vec<&str> = normalized_path
        .split('.')
        .filter(|s| !s.is_empty())
        .collect();

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
            Value::Array(arr) => {
                if let Ok(idx) = part.parse::<usize>() {
                    if let Some(val) = arr.get(idx) {
                        current = val;
                    } else {
                        return Err(RupostError::Other(format!(
                            "Index '{}' out of bounds in path '{}'",
                            idx, path
                        )));
                    }
                } else {
                    return Err(RupostError::Other(format!(
                        "Cannot use non-numeric key '{}' on an array in path '{}'",
                        part, path
                    )));
                }
            }
            _ => {
                return Err(RupostError::Other(format!(
                    "Cannot navigate path '{}' on non-object/array value",
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
    use crate::variable::VariableContext;

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

    #[test]
    fn test_capture_json_array_dot_index() {
        let body = r#"{"data": {"items": [{"id": 1}, {"id": 2}]}}"#;
        let headers = HeaderMap::new();
        let captures = vec![VariableCapture::from_body("second_id", "data.items.1.id")];

        let vars = capture_from_response(body, &headers, &captures).unwrap();
        assert_eq!(vars.get("second_id").unwrap(), "2");
    }

    #[test]
    fn test_capture_json_array_bracket() {
        let body = r#"{"data": {"items": [{"id": 1}, {"id": 2}]}}"#;
        let headers = HeaderMap::new();
        let captures = vec![VariableCapture::from_body("first_id", "data.items[0].id")];

        let vars = capture_from_response(body, &headers, &captures).unwrap();
        assert_eq!(vars.get("first_id").unwrap(), "1");
    }

    #[test]
    fn test_capture_regex_from_html() {
        let body = r#"
            <html>
                <body>
                    <input type="hidden" name="csrf_token" value="abc123xyz">
                </body>
            </html>
        "#;
        let headers = HeaderMap::new();

        // 测试有捕获组
        let capture1 = VariableCapture::parse(
            "token1",
            r#"regex <input type="hidden" name="csrf_token" value="([^"]+)">"#,
        );
        // 测试无捕获组
        let capture2 = VariableCapture::parse("token2", r#"regex name="csrf_token""#);

        let vars = capture_from_response(body, &headers, &[capture1, capture2]).unwrap();
        assert_eq!(vars.get("token1").unwrap(), "abc123xyz");
        assert_eq!(vars.get("token2").unwrap(), r#"name="csrf_token""#);
    }

    #[test]
    fn test_capture_regex_no_match() {
        let body = "Plain text body";
        let headers = HeaderMap::new();
        let captures = vec![VariableCapture::parse("token", "regex [0-9]+")];

        let result = capture_from_response(body, &headers, &captures);
        assert!(result.is_err());
    }

    #[test]
    fn test_capture_normal() {
        let mut ctx = VariableContext::new();
        let body = r#"{"data": "normal"}"#;
        let headers = HeaderMap::new();
        let captures = vec![VariableCapture::from_body("my_var", "data")];

        VariableCapture::capture_normal(&captures, body, &headers, &mut ctx);
        assert_eq!(ctx.get("my_var").unwrap(), "normal");
    }

    #[test]
    fn test_capture_sse_frame() {
        let mut ctx = VariableContext::new();
        let virtual_body = r#"{"data": "stream-delta"}"#;
        let mut virtual_headers = HeaderMap::new();
        virtual_headers.insert("x-sse-event", "message".parse().unwrap());
        virtual_headers.insert("x-sse-id", "123".parse().unwrap());

        let captures = vec![
            VariableCapture::from_body("event_name", "stream.event"),
            VariableCapture::from_body("id", "stream.id"),
            VariableCapture::from_body("delta", "stream.body.data"),
        ];

        VariableCapture::capture_sse_frame(&captures, virtual_body, &virtual_headers, &mut ctx);
        assert_eq!(ctx.get("event_name").unwrap(), "message");
        assert_eq!(ctx.get("id").unwrap(), "123");
        assert_eq!(ctx.get("delta").unwrap(), "stream-delta");
    }

    #[test]
    fn test_capture_sse_llm_content() {
        let mut ctx = VariableContext::new();
        let final_body = "accumulated body";
        let mut final_headers = HeaderMap::new();
        let encoded = url::form_urlencoded::byte_serialize(b"full content").collect::<String>();
        final_headers.insert("x-sse-llm-content", encoded.parse().unwrap());

        let captures = vec![VariableCapture::from_body("reply", "stream.llm.content")];

        VariableCapture::capture_sse_llm_content(&captures, final_body, &final_headers, &mut ctx);
        assert_eq!(ctx.get("reply").unwrap(), "full content");
    }
}
