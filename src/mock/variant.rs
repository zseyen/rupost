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

fn resolve_jsonpath(body: &str, path: &str) -> Option<serde_json::Value> {
    let val: serde_json::Value = serde_json::from_str(body).ok()?;
    if !path.starts_with('$') {
        return None;
    }

    let segments: Vec<&str> = path.split('.').skip(1).collect(); // skip '$'
    let mut current = &val;
    for seg in segments {
        // Handle array index shorthand like tags[0]
        if let Some(open_idx) = seg.find('[')
            && let Some(close_idx) = seg.find(']')
        {
            let field_name = &seg[..open_idx];
            let index_str = &seg[open_idx + 1..close_idx];
            let index: usize = index_str.parse().ok()?;

            if !field_name.is_empty() {
                current = current.get(field_name)?;
            }
            current = current.get(index)?;
        } else {
            current = current.get(seg)?;
        }
    }
    Some(current.clone())
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
                    headers.iter().find(|(k, _)| k.to_lowercase() == lower_key).map(|(_, v)| v)
                });

                if self.operator == CompareOp::Exists && self.expected_value == "None" {
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

                if self.operator == CompareOp::Exists && self.expected_value == "None" {
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
                let json_val = match resolve_jsonpath(body, &self.key) {
                    Some(v) => v,
                    None => return false,
                };

                match self.operator {
                    CompareOp::Exists => !json_val.is_null(),
                    CompareOp::Equals => match &json_val {
                        serde_json::Value::String(s) => s == &self.expected_value,
                        other => *other == self.expected_value,
                    },
                    CompareOp::Contains => match &json_val {
                        serde_json::Value::String(s) => s.contains(&self.expected_value),
                        serde_json::Value::Array(arr) => arr.iter().any(|item| match item {
                            serde_json::Value::String(s) => s == &self.expected_value,
                            other => *other == self.expected_value,
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
}
