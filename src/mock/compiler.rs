use crate::mock::matcher::MockRouteConfig;
use crate::mock::variant::{CompareOp, ConditionSource, MockVariant, VariantCondition};
use crate::parser::types::ParsedFile;
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
                let condition = var
                    .condition_expr
                    .as_ref()
                    .and_then(|expr| Self::parse_condition_expression(expr));

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

        let (source, key) = if let Some(stripped) = lhs.strip_prefix("query.") {
            (ConditionSource::Query, stripped.to_string())
        } else if let Some(stripped) = lhs.strip_prefix("header.") {
            (ConditionSource::Header, stripped.to_string())
        } else if let Some(stripped) = lhs.strip_prefix("body.") {
            (ConditionSource::Body, stripped.to_string())
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
}
