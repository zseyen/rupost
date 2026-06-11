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

use crate::mock::matcher::MockRouteConfig;
use crate::mock::variant::{MockVariant, VariantCondition, ConditionSource, CompareOp};
use crate::parser::types::ParsedMockVariant;
use std::collections::HashMap;

/// 契约/Mock 编译器
pub struct MockCompiler;

impl MockCompiler {
    /// 将解析后的 ParsedFile 转换为 Mock 引擎所接收的 MockRouteConfig 向量
    pub fn compile(parsed_file: &ParsedFile) -> Vec<MockRouteConfig> {
        let mut routes = Vec::new();

        // 过滤非测试块，只编译契约块
        for req in &parsed_file.requests {
            if req.metadata.is_test {
                continue;
            }

            let path = req.url.clone();
            let method = req.method_or_default().to_string();

            let mut variants = Vec::new();
            for var in &req.metadata.mock_variants {
                let condition = var.condition_expr.as_ref().and_then(|expr| {
                    Self::parse_condition_expression(expr)
                });

                let mut headers = HashMap::new();
                for (key, val) in &var.headers {
                    headers.insert(key.clone(), val.clone());
                }

                variants.push(MockVariant {
                    condition,
                    status: var.status,
                    headers,
                    response_body: var.body.clone().unwrap_or_default(),
                });
            }

            // 如果 mock_variants 为空，则把请求的默认 Body 提取为兜底 Variant
            if variants.is_empty() {
                let mut headers = HashMap::new();
                for (key, val) in &req.headers {
                    headers.insert(key.clone(), val.clone());
                }
                variants.push(MockVariant {
                    condition: None,
                    status: 200,
                    headers,
                    response_body: req.body.clone().unwrap_or_default(),
                });
            }

            routes.push(MockRouteConfig {
                method,
                path,
                variants,
            });
        }
        routes
    }

    /// 将表达式解析为 VariantCondition
    /// 例如: "query.role == admin" -> Query, key="role", Equals, expected="admin"
    pub fn parse_condition_expression(expr: &str) -> Option<VariantCondition> {
        let parts: Vec<&str> = expr.split("==").map(|s| s.trim()).collect();
        if parts.len() != 2 {
            return None;
        }
        let lhs = parts[0];
        let rhs_val = parts[1].trim_matches('"').trim();

        let (source, key) = if lhs.starts_with("query.") {
            (ConditionSource::Query, lhs["query.".len()..].to_string())
        } else if lhs.starts_with("header.") {
            (ConditionSource::Header, lhs["header.".len()..].to_string())
        } else if lhs.starts_with("body.") {
            (ConditionSource::Body, lhs["body.".len()..].to_string())
        } else {
            return None;
        };

        if rhs_val.eq_ignore_ascii_case("None") {
            Some(VariantCondition {
                source,
                key,
                operator: CompareOp::Exists,
                expected_value: "None".to_string(),
            })
        } else {
            Some(VariantCondition {
                source,
                key,
                operator: CompareOp::Equals,
                expected_value: rhs_val.to_string(),
            })
        }
    }
}

use crate::parser::types::ParsedFile;

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

    #[test]
    fn test_mock_compiler_compile() {
        let mut parsed_file = ParsedFile::new();

        // 1. 测试用例块 (应被 compile 过滤)
        let mut req_test = ParsedRequest::new(1);
        req_test.url = "/api/test".to_string();
        req_test.metadata.is_test = true;
        parsed_file.add_request(req_test);

        // 2. Mock 契约块 (带变体)
        let mut req_mock = ParsedRequest::new(5);
        req_mock.url = "/api/v1/orders".to_string();
        req_mock.method = Some("POST".to_string());
        req_mock.metadata.is_test = false;

        req_mock.metadata.mock_variants.push(ParsedMockVariant {
            condition_expr: Some("header.X-App-Version == 2.0.0".to_string()),
            status: 201,
            headers: vec![("Content-Type".to_string(), "application/json".to_string())],
            body: Some(r#"{"status": "created"}"#.to_string()),
        });

        req_mock.metadata.mock_variants.push(ParsedMockVariant {
            condition_expr: Some("header.Authorization == None".to_string()),
            status: 401,
            headers: Vec::new(),
            body: Some("Unauthorized".to_string()),
        });

        parsed_file.add_request(req_mock);

        let routes = MockCompiler::compile(&parsed_file);

        // 期望只剩 1 个路由 (过滤了 test 请求)
        assert_eq!(routes.len(), 1);
        let route = &routes[0];
        assert_eq!(route.path, "/api/v1/orders");
        assert_eq!(route.method, "POST");
        assert_eq!(route.variants.len(), 2);

        // 验证变体一
        let var1 = &route.variants[0];
        assert_eq!(var1.status, 201);
        assert_eq!(var1.response_body, r#"{"status": "created"}"#);
        let cond1 = var1.condition.as_ref().unwrap();
        assert_eq!(cond1.source, ConditionSource::Header);
        assert_eq!(cond1.key, "X-App-Version");
        assert_eq!(cond1.operator, CompareOp::Equals);
        assert_eq!(cond1.expected_value, "2.0.0");

        // 验证变体二 (None 反向匹配)
        let var2 = &route.variants[1];
        assert_eq!(var2.status, 401);
        let cond2 = var2.condition.as_ref().unwrap();
        assert_eq!(cond2.source, ConditionSource::Header);
        assert_eq!(cond2.key, "Authorization");
        assert_eq!(cond2.operator, CompareOp::Exists);
        assert_eq!(cond2.expected_value, "None");
    }
}
