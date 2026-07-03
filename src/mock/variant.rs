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

fn resolve_jsonpath(body: &str, path: &str) -> Option<serde_json::Value> {
    let val: serde_json::Value = serde_json::from_str(body).ok()?;
    crate::utils::jsonpath::JsonPathResolver::resolve(&val, path).cloned()
}

impl VariantCondition {
    pub fn evaluate(
        &self,
        headers: &HashMap<String, String>,
        query: &HashMap<String, String>,
        body: &str,
    ) -> bool {
        match self.source {
            ConditionSource::Header => {
                let val_opt = headers.get(&self.key).or_else(|| {
                    let lower_key = self.key.to_lowercase();
                    headers
                        .iter()
                        .find(|(k, _)| k.to_lowercase() == lower_key)
                        .map(|(_, v)| v)
                });

                if self.operator == CompareOp::Exists && is_none_value(&self.expected_value) {
                    return val_opt.is_none();
                }

                let val = match val_opt {
                    Some(v) => v,
                    None => return false,
                };

                match self.operator {
                    CompareOp::Exists => true,
                    CompareOp::Equals => val == &self.expected_value,
                    CompareOp::Contains => val.contains(&self.expected_value),
                }
            }
            ConditionSource::Query => {
                let val_opt = query.get(&self.key);

                if self.operator == CompareOp::Exists && is_none_value(&self.expected_value) {
                    return val_opt.is_none();
                }

                let val = match val_opt {
                    Some(v) => v,
                    None => return false,
                };

                match self.operator {
                    CompareOp::Exists => true,
                    CompareOp::Equals => val == &self.expected_value,
                    CompareOp::Contains => val.contains(&self.expected_value),
                }
            }
            ConditionSource::Body => {
                let json_val_opt = resolve_jsonpath(body, &self.key);

                if self.operator == CompareOp::Exists && is_none_value(&self.expected_value) {
                    return json_val_opt.is_none() || json_val_opt.unwrap().is_null();
                }

                let json_val = match json_val_opt {
                    Some(v) => v,
                    None => return false,
                };

                #[allow(clippy::cmp_owned)]
                match self.operator {
                    CompareOp::Exists => !json_val.is_null(),
                    CompareOp::Equals => match &json_val {
                        serde_json::Value::String(s) => s == &self.expected_value,
                        other => {
                            *other == self.expected_value
                                || other.to_string() == self.expected_value
                        }
                    },
                    CompareOp::Contains => match &json_val {
                        serde_json::Value::String(s) => s.contains(&self.expected_value),
                        serde_json::Value::Array(arr) => arr.iter().any(|item| match item {
                            serde_json::Value::String(s) => s == &self.expected_value,
                            other => {
                                *other == self.expected_value
                                    || other.to_string() == self.expected_value
                            }
                        }),
                        other => other.to_string().contains(&self.expected_value),
                    },
                }
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
}
