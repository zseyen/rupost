use crate::http::Request;
use crate::parser::ParsedRequest;
use crate::{Result, RupostError};

/// 将 ParsedRequest 转换为可执行的 Request
impl TryFrom<ParsedRequest> for Request {
    type Error = RupostError;

    fn try_from(parsed: ParsedRequest) -> Result<Self> {
        // 1. 获取方法（默认 GET）
        let method = parsed.method_or_default();

        // 2. 创建基础请求
        let mut request = Request::new(method, &parsed.url)?;

        // 3. 添加 headers
        for (key, value) in &parsed.headers {
            request = request.with_header(key, value);
        }

        // 4. 添加 body（自动推断类型）
        if let Some(body) = &parsed.body {
            request = add_body(request, body, &parsed.headers)?;
        }

        Ok(request)
    }
}

/// Body 类型推断和添加
fn add_body(mut request: Request, body: &str, headers: &[(String, String)]) -> Result<Request> {
    // 检查 Content-Type header
    let content_type = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.as_str());

    match content_type {
        Some(ct) if ct.contains("application/json") => {
            // 显式指定 JSON：验证格式并使用 with_json
            let json: serde_json::Value = serde_json::from_str(body)?;
            request = request.with_json(&json)?;
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
