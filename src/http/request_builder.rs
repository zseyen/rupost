use crate::http::Request;
use crate::parser::ParsedRequest;
use crate::{Result, RupostError};

/// 将 ParsedRequest 转换为可执行的 Request，同时返回构建期的非致命警告。
///
/// 目前唯一的警告来源：`Content-Type: application/json` 但 body 不是合法 JSON——
/// 此时按原文发送并保留用户声明的 Content-Type（测试工具必须能把坏负载发给
/// 服务端验证其 400/422 处理，此前这会被本地直接拒绝）。
pub fn try_from_parsed_with_warnings(parsed: ParsedRequest) -> Result<(Request, Vec<String>)> {
    let mut warnings = Vec::new();
    // 1. 获取方法（默认 GET）
    let method = parsed.method_or_default();

    // 2. 创建基础请求
    let mut request = Request::new(method, &parsed.url)?;

    // 3. 添加 headers
    for (key, value) in &parsed.headers {
        request = request.with_header(key, value)?;
    }

    // 4. 添加 body（自动推断类型）
    if let Some(body) = &parsed.body {
        request = add_body(request, body, &parsed.headers, &mut warnings)?;
    }

    // 5. @timeout：仅对非流式请求生效。SSE（@sse 或 Accept: text/event-stream）
    //    与 WebSocket 是长连接，reqwest 的 per-request timeout 是总时长语义，
    //    套上去会把本应长连的流在中途切断；SSE 有专属 @sse_timeout。
    let accepts_event_stream = parsed
        .headers
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case("accept") && v.contains("text/event-stream"));
    let is_streaming = parsed.metadata.sse || parsed.metadata.websocket || accepts_event_stream;
    if !is_streaming {
        request = request.with_timeout(parsed.metadata.timeout);
    }

    Ok((request, warnings))
}

impl Request {
    /// 便捷入口：只要 Request、不关心警告（警告被丢弃）
    pub fn try_from_parsed(parsed: ParsedRequest) -> Result<Self> {
        Ok(crate::http::try_from_parsed_with_warnings(parsed)?.0)
    }
}

impl TryFrom<ParsedRequest> for Request {
    type Error = RupostError;

    fn try_from(parsed: ParsedRequest) -> Result<Self> {
        Request::try_from_parsed(parsed)
    }
}

/// Body 类型推断和添加
fn add_body(
    mut request: Request,
    body: &str,
    headers: &[(String, String)],
    warnings: &mut Vec<String>,
) -> Result<Request> {
    // 检查 Content-Type header
    let content_type = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.as_str());

    match content_type {
        Some(ct) if ct.contains("application/json") => {
            // 显式指定 JSON：能解析就用 with_json；畸形时按原文发送
            match serde_json::from_str::<serde_json::Value>(body) {
                Ok(json) => {
                    request = request.with_json(&json)?;
                }
                Err(e) => {
                    warnings.push(format!(
                        "body 不是合法 JSON（{e}），已按原文发送（Content-Type 声明保留）"
                    ));
                    request = request.with_text(body);
                }
            }
        }
        Some(ct) if ct.contains("application/x-www-form-urlencoded") => {
            // 显式指定 Form：直接使用文本
            request = request.with_text(body);
        }
        _ => {
            // 未指定 Content-Type：尝试自动检测
            if is_json_like(body) {
                // 看起来像 JSON，尝试解析
                match serde_json::from_str::<serde_json::Value>(body) {
                    Ok(json) => {
                        request = request.with_json(&json)?;
                    }
                    Err(_) => {
                        // JSON 解析失败，使用纯文本
                        request = request.with_text(body);
                    }
                }
            } else {
                // 不像 JSON，使用纯文本
                request = request.with_text(body);
            }
        }
    }

    Ok(request)
}

/// 简单的 JSON 格式检测
fn is_json_like(s: &str) -> bool {
    let trimmed = s.trim();
    (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
}

/// 便捷函数：从 ParsedRequest 构建 Request
pub fn to_request(parsed: ParsedRequest) -> Result<Request> {
    parsed.try_into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ParsedRequest;
    use std::time::Duration;

    #[test]
    fn test_timeout_propagates_for_plain_requests() {
        // 回归：@timeout 此前只被解析、从未应用到 HTTP 请求（client 硬编码 30s）
        let mut parsed = ParsedRequest::new(1);
        parsed.url = "http://example.com".to_string();
        parsed.metadata.timeout = Some(Duration::from_millis(500));

        let request: Request = parsed.try_into().unwrap();
        assert_eq!(request.timeout, Some(Duration::from_millis(500)));

        // 未声明 @timeout 时保持 None（走 client 级默认）
        let mut parsed = ParsedRequest::new(1);
        parsed.url = "http://example.com".to_string();
        let request: Request = parsed.try_into().unwrap();
        assert_eq!(request.timeout, None);
    }

    #[test]
    fn test_malformed_json_body_falls_back_to_text_with_warning() {
        // 回归：显式 application/json + 畸形 body 此前被本地直接拒绝，
        // 无法故意发送坏负载验证服务端的 400/422 处理
        let mut parsed = ParsedRequest::new(1);
        parsed.url = "http://example.com".to_string();
        parsed.headers = vec![("Content-Type".to_string(), "application/json".to_string())];
        parsed.body = Some("{\"broken\": ".to_string());

        let (request, warnings) = try_from_parsed_with_warnings(parsed).unwrap();
        assert!(request.body.is_some(), "畸形 body 应按原文发送而非丢弃");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("不是合法 JSON"), "{warnings:?}");
        // 用户声明的 Content-Type 保留
        assert_eq!(
            request.headers.get("content-type").unwrap(),
            "application/json"
        );
    }

    #[test]
    fn test_valid_json_body_produces_no_warning() {
        let mut parsed = ParsedRequest::new(1);
        parsed.url = "http://example.com".to_string();
        parsed.headers = vec![("Content-Type".to_string(), "application/json".to_string())];
        parsed.body = Some(r#"{"ok": true}"#.to_string());

        let (_request, warnings) = try_from_parsed_with_warnings(parsed).unwrap();
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_timeout_exempted_for_streaming_requests() {
        // reqwest 的 per-request timeout 是总时长语义，套在长连流上会误杀：
        // @sse / @websocket / Accept: text/event-stream 三种流式信号均豁免
        let mut parsed = ParsedRequest::new(1);
        parsed.url = "http://example.com/stream".to_string();
        parsed.metadata.timeout = Some(Duration::from_secs(5));
        parsed.metadata.sse = true;
        let request: Request = parsed.try_into().unwrap();
        assert_eq!(request.timeout, None);

        let mut parsed = ParsedRequest::new(1);
        parsed.url = "http://example.com/ws".to_string();
        parsed.metadata.timeout = Some(Duration::from_secs(5));
        parsed.metadata.websocket = true;
        let request: Request = parsed.try_into().unwrap();
        assert_eq!(request.timeout, None);

        let mut parsed = ParsedRequest::new(1);
        parsed.url = "http://example.com/events".to_string();
        parsed.metadata.timeout = Some(Duration::from_secs(5));
        parsed
            .headers
            .push(("Accept".to_string(), "text/event-stream".to_string()));
        let request: Request = parsed.try_into().unwrap();
        assert_eq!(request.timeout, None);
    }

    #[test]
    fn test_convert_simple_get() {
        let mut parsed = ParsedRequest::new(1);
        parsed.url = "http://example.com".to_string();

        let request: Request = parsed.try_into().unwrap();
        assert_eq!(request.method.as_str(), "GET");
        assert_eq!(request.url.to_string(), "http://example.com:80/");
    }

    #[test]
    fn test_convert_with_method() {
        let mut parsed = ParsedRequest::new(1);
        parsed.method = Some("POST".to_string());
        parsed.url = "http://example.com".to_string();

        let request: Request = parsed.try_into().unwrap();
        assert_eq!(request.method.as_str(), "POST");
    }

    #[test]
    fn test_convert_with_headers() {
        let mut parsed = ParsedRequest::new(1);
        parsed.url = "http://example.com".to_string();
        parsed
            .headers
            .push(("User-Agent".to_string(), "RuPost/1.0".to_string()));
        parsed
            .headers
            .push(("Content-Type".to_string(), "application/json".to_string()));

        let request: Request = parsed.try_into().unwrap();
        assert!(request.headers.contains_key("user-agent"));
        assert!(request.headers.contains_key("content-type"));
    }

    #[test]
    fn test_convert_with_json_body() {
        let mut parsed = ParsedRequest::new(1);
        parsed.url = "http://example.com".to_string();
        parsed.body = Some(r#"{"name": "test", "value": 123}"#.to_string());

        let request: Request = parsed.try_into().unwrap();
        assert!(request.body.is_some());
    }

    #[test]
    fn test_convert_with_text_body() {
        let mut parsed = ParsedRequest::new(1);
        parsed.url = "http://example.com".to_string();
        parsed.body = Some("plain text data".to_string());

        let request: Request = parsed.try_into().unwrap();
        assert!(request.body.is_some());
    }

    #[test]
    fn test_is_json_like() {
        assert!(is_json_like(r#"{"key": "value"}"#));
        assert!(is_json_like(r#"  {"key": "value"}  "#)); // 带空格
        assert!(is_json_like(r#"[1, 2, 3]"#));
        assert!(is_json_like(r#"  [1, 2, 3]  "#)); // 带空格
        assert!(!is_json_like("plain text"));
        assert!(!is_json_like("key=value"));
    }

    #[test]
    fn test_auto_detect_json() {
        let mut parsed = ParsedRequest::new(1);
        parsed.url = "http://example.com".to_string();
        // 没有 Content-Type，但 body 看起来像 JSON
        parsed.body = Some(r#"{"auto": "detect"}"#.to_string());

        let request: Request = parsed.try_into().unwrap();
        assert!(request.body.is_some());
        // 应该被自动识别为 JSON 并添加 Content-Type header
        assert!(request.headers.contains_key("content-type"));
    }

    #[test]
    fn test_invalid_url() {
        let mut parsed = ParsedRequest::new(1);
        parsed.url = "not a valid url".to_string();

        let result: Result<Request> = parsed.try_into();
        assert!(result.is_err());
    }
}
