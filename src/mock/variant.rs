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
