use crate::Result;
use crate::error::RupostError;
use crate::mock::matcher::MockRouteConfig;
use crate::mock::variant::{CompareOp, ConditionSource, MockVariant, VariantCondition};
use crate::parser::types::ParsedFile;
use std::collections::HashMap;

/// 契约/Mock 编译器
pub struct MockCompiler;

impl MockCompiler {
    /// 严格编译解析后的 ParsedFile 为 MockRouteConfig 向量：
    /// 若 `@mock-when` 条件表达式无效则返回 ParseError，避免静默退化为无条件兜底变体；
    /// 同时稳定保证有条件的变体排在无条件兜底变体之前。
    pub fn try_compile(parsed_file: &ParsedFile) -> Result<Vec<MockRouteConfig>> {
        Self::compile_inner(parsed_file, true)
    }

    /// 将解析后的 ParsedFile 转换为 Mock 引擎所接收的 MockRouteConfig 向量
    pub fn compile(parsed_file: &ParsedFile) -> Vec<MockRouteConfig> {
        Self::compile_inner(parsed_file, false).unwrap_or_default()
    }

    fn compile_inner(parsed_file: &ParsedFile, strict: bool) -> Result<Vec<MockRouteConfig>> {
        let mut routes = Vec::new();

        // 双角色语义：是否编译进 mock 服务器由「是否声明 Mock 变体」决定，与 @test 无关。
        // 无变体的纯契约/测试块不注册路由（守护测试：markdown_integration_test 双角色断言）
        for req in &parsed_file.requests {
            if req.metadata.mock_variants.is_empty() {
                continue;
            }

            let path = Self::extract_route_path(&req.url);
            let method = req.method_or_default().to_string();

            let mut variants = Vec::new();
            for var in &req.metadata.mock_variants {
                let condition = match var.condition_expr.as_deref() {
                    Some(expr) => match Self::parse_condition_expression(expr) {
                        Some(cond) => Some(cond),
                        None if strict => {
                            return Err(RupostError::ParseError(format!(
                                "Invalid @mock-when condition expression: '{expr}'"
                            )));
                        }
                        None => {
                            tracing::warn!(
                                "Skipping invalid @mock-when condition expression: '{}'",
                                expr
                            );
                            continue;
                        }
                    },
                    None => None,
                };

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

            // 保持有条件变体相对顺序，仅将无条件兜底变体 (@mock-default) 稳定沉底
            variants.sort_by_key(|v| v.condition.is_none());

            routes.push(MockRouteConfig {
                method,
                path,
                variants,
            });
        }
        Ok(routes)
    }

    /// 纯词法提取规范化路由路径（保留 `:id` 与 `{id}`，不做 URL 百分号编码）：
    /// 剥离 `?query`、`#fragment`、前导 `{{...}}` 变量前缀及 `scheme://host`。
    pub fn extract_route_path(raw_url: &str) -> String {
        let trimmed = raw_url.trim();
        let without_frag = trimmed.split('#').next().unwrap_or(trimmed);
        let without_query = without_frag.split('?').next().unwrap_or(without_frag).trim();

        // 剥离前导模板变量（例如 "{{base_url}}/api/users/:id"）
        let after_var = if let Some(rest) = without_query.strip_prefix("{{")
            && let Some(end_brace) = rest.find("}}")
        {
            rest[end_brace + 2..].trim()
        } else {
            without_query
        };

        // 剥离 scheme://host（例如 "http://localhost:9000/api/orders/{id}"）
        let path_part = if let Some(scheme_pos) = after_var.find("://") {
            let after_scheme = &after_var[scheme_pos + 3..];
            match after_scheme.find('/') {
                Some(slash_pos) => &after_scheme[slash_pos..],
                None => "/",
            }
        } else {
            after_var
        };

        if path_part.is_empty() {
            "/".to_string()
        } else if path_part.starts_with('/') {
            path_part.to_string()
        } else {
            format!("/{path_part}")
        }
    }

    /// 将表达式解析为 VariantCondition
    /// 例如: "query.role == admin" -> Query, key="role", Equals, expected="admin"
    pub fn parse_condition_expression(expr: &str) -> Option<VariantCondition> {
        let (lhs_raw, rhs_raw) = expr.split_once("==")?;
        let lhs = lhs_raw.trim();
        let rhs_val = rhs_raw.trim().trim_matches('"').trim();

        let (source, stripped) = lhs
            .strip_prefix("query.")
            .map(|s| (ConditionSource::Query, s))
            .or_else(|| {
                lhs.strip_prefix("header.")
                    .map(|s| (ConditionSource::Header, s))
            })
            .or_else(|| {
                lhs.strip_prefix("body.")
                    .map(|s| (ConditionSource::Body, s))
            })?;
        let key = stripped.trim().to_string();
        if key.is_empty() {
            return None;
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ParsedRequest;
    use crate::parser::types::ParsedMockVariant;

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

    #[test]
    fn test_parse_condition_expression_value_with_double_equals() {
        let cond = MockCompiler::parse_condition_expression(
            "header.Authorization == \"Bearer abc==\"",
        )
        .expect("should parse value containing '=='");
        assert_eq!(cond.source, ConditionSource::Header);
        assert_eq!(cond.key, "Authorization");
        assert_eq!(cond.operator, CompareOp::Equals);
        assert_eq!(cond.expected_value, "Bearer abc==");
    }

    #[test]
    fn test_try_compile_rejects_invalid_condition_without_silent_fallback() {
        let mut parsed_file = ParsedFile::new();
        let mut req = ParsedRequest::new(1);
        req.url = "/api/v1/items".to_string();
        req.metadata.mock_variants.push(ParsedMockVariant {
            condition_expr: Some("invalid_condition_syntax".to_string()),
            status: 200,
            headers: Vec::new(),
            body: Some("ok".to_string()),
        });
        parsed_file.add_request(req);

        let err = MockCompiler::try_compile(&parsed_file).unwrap_err();
        assert!(err.to_string().contains("invalid_condition_syntax"));
    }

    #[test]
    fn test_try_compile_orders_default_variant_after_conditional() {
        let mut parsed_file = ParsedFile::new();
        let mut req = ParsedRequest::new(1);
        req.url = "/api/v1/items".to_string();
        req.metadata.mock_variants.push(ParsedMockVariant {
            condition_expr: None,
            status: 403,
            headers: Vec::new(),
            body: Some("default".to_string()),
        });
        req.metadata.mock_variants.push(ParsedMockVariant {
            condition_expr: Some("query.role == admin".to_string()),
            status: 200,
            headers: Vec::new(),
            body: Some("admin".to_string()),
        });
        parsed_file.add_request(req);

        let routes = MockCompiler::try_compile(&parsed_file).unwrap();
        assert_eq!(routes[0].variants[0].status, 200);
        assert!(routes[0].variants[0].condition.is_some());
        assert_eq!(routes[0].variants[1].status, 403);
        assert!(routes[0].variants[1].condition.is_none());
    }

    #[test]
    fn test_extract_route_path_preserves_braces_and_strips_base_url() {
        assert_eq!(
            MockCompiler::extract_route_path("{{base_url}}/api/users/:id?debug=1#top"),
            "/api/users/:id"
        );
        assert_eq!(
            MockCompiler::extract_route_path("http://localhost:9000/api/orders/{id}?a=1"),
            "/api/orders/{id}"
        );
        assert_eq!(
            MockCompiler::extract_route_path("https://example.com"),
            "/"
        );
        assert_eq!(
            MockCompiler::extract_route_path("api/v1/ping?x=1"),
            "/api/v1/ping"
        );
    }
}
