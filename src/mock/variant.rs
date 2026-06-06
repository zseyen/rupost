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
    pub key: String,             // JSONPath like $.user.role or Header Name
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

impl VariantCondition {
    pub fn evaluate(
        &self,
        headers: &HashMap<String, String>,
        query: &HashMap<String, String>,
        body: &str,
    ) -> bool {
        let _ = headers;
        let _ = query;
        let _ = body;
        todo!()
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
