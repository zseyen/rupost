use crate::utils::jsonpath::JsonPathResolver;
use serde_json::Value;
use std::cell::OnceCell;
use std::collections::HashMap;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum ConditionSource {
    Header,
    Query,
    Body,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum CompareOp {
    Equals,
    Contains,
    Exists,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct VariantCondition {
    pub source: ConditionSource,
    pub key: String, // JSONPath like $.user.role or Header Name
    pub operator: CompareOp,
    pub expected_value: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MockVariant {
    pub condition: Option<VariantCondition>,
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub response_body: String,
}

fn is_none_value(val: &str) -> bool {
    let variants = ["None", "null", "NULL", "nil", "undefined"];
    variants.contains(&val)
}

/// 单次请求的求值上下文（惰性缓存 JSON 解析结果，避免多变体重复反序列化）
pub struct EvalContext<'a> {
    pub headers: &'a HashMap<String, String>,
    pub query: &'a HashMap<String, String>,
    pub body: &'a str,
    parsed_json: OnceCell<Option<Value>>,
}

impl<'a> EvalContext<'a> {
    pub fn new(
        headers: &'a HashMap<String, String>,
        query: &'a HashMap<String, String>,
        body: &'a str,
    ) -> Self {
        Self {
            headers,
            query,
            body,
            parsed_json: OnceCell::new(),
        }
    }

    pub fn json_body(&self) -> Option<&Value> {
        self.parsed_json
            .get_or_init(|| serde_json::from_str(self.body).ok())
            .as_ref()
    }
}

/// 统一的诊断评估结果，供运行时与 Web 预演器共用
pub struct ConditionEvalDetail {
    pub passed: bool,
    pub actual: Option<String>,
}

fn eval_str_op(actual: Option<&str>, operator: &CompareOp, expected: &str) -> bool {
    if *operator == CompareOp::Exists && is_none_value(expected) {
        return actual.is_none();
    }
    let Some(val) = actual else {
        return false;
    };
    match operator {
        CompareOp::Exists => true,
        CompareOp::Equals => val == expected,
        CompareOp::Contains => val.contains(expected),
    }
}

fn eval_json_op(json_val: Option<&Value>, operator: &CompareOp, expected: &str) -> bool {
    if *operator == CompareOp::Exists && is_none_value(expected) {
        return json_val.is_none_or(|v| v.is_null());
    }
    let Some(val) = json_val else {
        return false;
    };
    #[allow(clippy::cmp_owned)]
    match operator {
        CompareOp::Exists => !val.is_null(),
        CompareOp::Equals => match val {
            Value::String(s) => s == expected,
            other => *other == expected || other.to_string() == expected,
        },
        CompareOp::Contains => match val {
            Value::String(s) => s.contains(expected),
            Value::Array(arr) => arr.iter().any(|item| match item {
                Value::String(s) => s == expected,
                other => *other == expected || other.to_string() == expected,
            }),
            other => other.to_string().contains(expected),
        },
    }
}

impl VariantCondition {
    pub fn evaluate(
        &self,
        headers: &HashMap<String, String>,
        query: &HashMap<String, String>,
        body: &str,
    ) -> bool {
        self.evaluate_in(&EvalContext::new(headers, query, body))
    }

    pub fn evaluate_in(&self, ctx: &EvalContext<'_>) -> bool {
        self.evaluate_with_detail(ctx).passed
    }

    pub fn evaluate_with_detail(&self, ctx: &EvalContext<'_>) -> ConditionEvalDetail {
        match self.source {
            ConditionSource::Header => {
                let actual = ctx
                    .headers
                    .get(&self.key)
                    .or_else(|| {
                        ctx.headers
                            .iter()
                            .find(|(k, _)| k.eq_ignore_ascii_case(&self.key))
                            .map(|(_, v)| v)
                    })
                    .cloned();
                let passed = eval_str_op(actual.as_deref(), &self.operator, &self.expected_value);
                ConditionEvalDetail { passed, actual }
            }
            ConditionSource::Query => {
                let actual = ctx.query.get(&self.key).cloned();
                let passed = eval_str_op(actual.as_deref(), &self.operator, &self.expected_value);
                ConditionEvalDetail { passed, actual }
            }
            ConditionSource::Body => {
                let json_val = ctx
                    .json_body()
                    .and_then(|root| JsonPathResolver::resolve(root, &self.key));
                let actual = json_val.map(|v| match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                });
                let passed = eval_json_op(json_val, &self.operator, &self.expected_value);
                ConditionEvalDetail { passed, actual }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_condition_header() {
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer token123".to_string());

        let cond1 = VariantCondition {
            source: ConditionSource::Header,
            key: "Authorization".to_string(),
            operator: CompareOp::Equals,
            expected_value: "Bearer token123".to_string(),
        };
        assert!(cond1.evaluate(&headers, &HashMap::new(), ""));

        let cond2 = VariantCondition {
            source: ConditionSource::Header,
            key: "Authorization".to_string(),
            operator: CompareOp::Contains,
            expected_value: "token123".to_string(),
        };
        assert!(cond2.evaluate(&headers, &HashMap::new(), ""));

        let cond3 = VariantCondition {
            source: ConditionSource::Header,
            key: "X-Requested-With".to_string(),
            operator: CompareOp::Exists,
            expected_value: "".to_string(),
        };
        assert!(!cond3.evaluate(&headers, &HashMap::new(), ""));
    }

    #[test]
    fn test_condition_query() {
        let mut query = HashMap::new();
        query.insert("role".to_string(), "admin".to_string());

        let cond = VariantCondition {
            source: ConditionSource::Query,
            key: "role".to_string(),
            operator: CompareOp::Equals,
            expected_value: "admin".to_string(),
        };
        assert!(cond.evaluate(&HashMap::new(), &query, ""));
    }

    #[test]
    fn test_condition_body_jsonpath() {
        let body = r#"{"user":{"role":"admin","tags":["premium","staff"]}}"#;

        // Equals
        let cond1 = VariantCondition {
            source: ConditionSource::Body,
            key: "$.user.role".to_string(),
            operator: CompareOp::Equals,
            expected_value: "admin".to_string(),
        };
        assert!(cond1.evaluate(&HashMap::new(), &HashMap::new(), body));

        // Contains
        let cond2 = VariantCondition {
            source: ConditionSource::Body,
            key: "$.user.tags".to_string(),
            operator: CompareOp::Contains,
            expected_value: "premium".to_string(),
        };
        assert!(cond2.evaluate(&HashMap::new(), &HashMap::new(), body));

        // Exists
        let cond3 = VariantCondition {
            source: ConditionSource::Body,
            key: "$.user.nonexistent".to_string(),
            operator: CompareOp::Exists,
            expected_value: "".to_string(),
        };
        assert!(!cond3.evaluate(&HashMap::new(), &HashMap::new(), body));
    }

    #[test]
    fn test_condition_body_jsonpath_array_index() {
        let body = r#"{"store":{"books":[{"title":"Book A"},{"title":"Book B"}]}}"#;

        // 验证点号数字定位 $.store.books.1.title
        let cond1 = VariantCondition {
            source: ConditionSource::Body,
            key: "$.store.books.1.title".to_string(),
            operator: CompareOp::Equals,
            expected_value: "Book B".to_string(),
        };
        assert!(cond1.evaluate(&HashMap::new(), &HashMap::new(), body));

        // 验证中括号定位 $.store.books[0].title
        let cond2 = VariantCondition {
            source: ConditionSource::Body,
            key: "$.store.books[0].title".to_string(),
            operator: CompareOp::Equals,
            expected_value: "Book A".to_string(),
        };
        assert!(cond2.evaluate(&HashMap::new(), &HashMap::new(), body));

        // 验证根数组定位 $[1].name
        let root_arr_body = r#"[{"name":"Alice"},{"name":"Bob"}]"#;
        let cond3 = VariantCondition {
            source: ConditionSource::Body,
            key: "$[1].name".to_string(),
            operator: CompareOp::Equals,
            expected_value: "Bob".to_string(),
        };
        assert!(cond3.evaluate(&HashMap::new(), &HashMap::new(), root_arr_body));
    }

    #[test]
    fn test_condition_none_and_null_semantics() {
        // 1. 验证 Header 缺失匹配 (X-Mock-Version exists null/None)
        let headers_empty = HashMap::new();
        let cond_header = VariantCondition {
            source: ConditionSource::Header,
            key: "X-Mock-Version".to_string(),
            operator: CompareOp::Exists,
            expected_value: "null".to_string(), // null 泛化空值
        };
        assert!(cond_header.evaluate(&headers_empty, &HashMap::new(), ""));

        // 2. 验证 Query 缺失匹配 (user exists undefined)
        let query_empty = HashMap::new();
        let cond_query = VariantCondition {
            source: ConditionSource::Query,
            key: "user".to_string(),
            operator: CompareOp::Exists,
            expected_value: "undefined".to_string(), // undefined 泛化空值
        };
        assert!(cond_query.evaluate(&HashMap::new(), &query_empty, ""));

        // 3. 验证 Body null/undefined 值匹配
        let body_null = r#"{"user": null}"#;
        let cond_body_null = VariantCondition {
            source: ConditionSource::Body,
            key: "$.user".to_string(),
            operator: CompareOp::Exists,
            expected_value: "None".to_string(), // None 泛化空值
        };
        assert!(cond_body_null.evaluate(&HashMap::new(), &HashMap::new(), body_null));

        let body_empty = r#"{}"#;
        assert!(cond_body_null.evaluate(&HashMap::new(), &HashMap::new(), body_empty));
    }

    #[test]
    fn test_eval_context_caches_json_and_returns_detail() {
        let headers = HashMap::new();
        let query = HashMap::new();
        let body = r#"{"user":{"role":"admin"}}"#;
        let ctx = EvalContext::new(&headers, &query, body);

        let cond = VariantCondition {
            source: ConditionSource::Body,
            key: "$.user.role".to_string(),
            operator: CompareOp::Equals,
            expected_value: "admin".to_string(),
        };
        let detail = cond.evaluate_with_detail(&ctx);
        assert!(detail.passed);
        assert_eq!(detail.actual.as_deref(), Some("admin"));
        assert!(ctx.json_body().is_some());
    }
}
