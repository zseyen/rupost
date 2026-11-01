use crate::client::AiClient;
use crate::error::{AiError, Result};
use crate::json_util::extract_json_block;
use serde::{Deserialize, Serialize};

const ATTRIBUTION_SYSTEM_PROMPT: &str = r#"You are a senior API testing diagnostic expert for the `rupost` CLI tool.
Analyze the failed HTTP request context and output ONLY a valid JSON object (no markdown fences, no extra prose) with this exact schema:
{
  "root_cause": "One concise sentence explaining why the request or assertion failed",
  "fix_suggestion": "Concrete actionable step to fix the .http file, environment variable, or backend service",
  "suggested_assert": "Optional updated @assert line if the assertion itself is outdated, or null"
}"#;

/// 失败断言摘要 DTO
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FailedAssertionInfo {
    pub raw: String,
    pub expected: Option<String>,
    pub actual: Option<String>,
    pub message: Option<String>,
}

/// 传入 `FailureAttributor` 的单次失败脱敏上下文（已由调用方完成脱敏与 512B 截断）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FailureContext {
    pub request_name: Option<String>,
    pub method: String,
    pub url: String,
    /// 网络/DNS/TLS 故障时为 None
    pub status_code: Option<u16>,
    pub error: Option<String>,
    pub failed_assertions: Vec<FailedAssertionInfo>,
    pub response_body_snippet: Option<String>,
    pub warnings: Vec<String>,
}

/// AI 单点失败归因输出（serde alias 兼容 PRD §6.2 的 summary/suggested_action 命名）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttributionAdvice {
    #[serde(alias = "summary")]
    pub root_cause: String,
    #[serde(alias = "suggested_action", default)]
    pub fix_suggestion: String,
    #[serde(default)]
    pub suggested_assert: Option<String>,
}

pub struct FailureAttributor<'a> {
    client: &'a dyn AiClient,
}

impl<'a> FailureAttributor<'a> {
    pub fn new(client: &'a dyn AiClient) -> Self {
        Self { client }
    }

    pub async fn attribute(&self, ctx: &FailureContext) -> Result<AttributionAdvice> {
        let user_json = serde_json::to_string_pretty(ctx)?;
        let raw_reply = self
            .client
            .generate_text(ATTRIBUTION_SYSTEM_PROMPT, &user_json)
            .await?;
        let clean_json = extract_json_block(&raw_reply);

        let advice: AttributionAdvice = serde_json::from_str(clean_json).map_err(|e| {
            AiError::TranslationFailed(format!(
                "Invalid JSON in attribution response ({e}): {clean_json}"
            ))
        })?;

        if advice.root_cause.trim().is_empty() {
            return Err(AiError::TranslationFailed(
                "Attribution root_cause cannot be empty".to_string(),
            ));
        }

        Ok(advice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use mockall::mock;

    mock! {
        pub Client {}
        #[async_trait]
        impl AiClient for Client {
            async fn generate_text(&self, system_prompt: &str, user_prompt: &str) -> Result<String>;
        }
    }

    fn sample_context() -> FailureContext {
        FailureContext {
            request_name: Some("GetUserProfile".to_string()),
            method: "GET".to_string(),
            url: "https://api.example.com/v1/profile".to_string(),
            status_code: Some(401),
            error: None,
            failed_assertions: vec![FailedAssertionInfo {
                raw: "status == 200".to_string(),
                expected: Some("200".to_string()),
                actual: Some("401".to_string()),
                message: Some("Status code mismatch".to_string()),
            }],
            response_body_snippet: Some(r#"{"error":"token_expired"}"#.to_string()),
            warnings: vec![],
        }
    }

    #[tokio::test]
    async fn test_attribute_parses_fenced_json_successfully() {
        let mut mock = MockClient::new();
        mock.expect_generate_text().times(1).returning(|_, _| {
            Ok("```json\n{\"root_cause\":\"JWT token expired\",\"fix_suggestion\":\"Refresh token via login endpoint first\",\"suggested_assert\":null}\n```".to_string())
        });

        let attributor = FailureAttributor::new(&mock);
        let advice = attributor.attribute(&sample_context()).await.unwrap();
        assert_eq!(advice.root_cause, "JWT token expired");
        assert_eq!(
            advice.fix_suggestion,
            "Refresh token via login endpoint first"
        );
        assert_eq!(advice.suggested_assert, None);
    }

    #[tokio::test]
    async fn test_attribute_accepts_prd_alias_fields() {
        // PRD §6.2 旧命名（summary/suggested_action）经 serde alias 无损兼容
        let mut mock = MockClient::new();
        mock.expect_generate_text().times(1).returning(|_, _| {
            Ok(r#"{"summary":"认证失败","suggested_action":"重新登录"}"#.to_string())
        });

        let attributor = FailureAttributor::new(&mock);
        let advice = attributor.attribute(&sample_context()).await.unwrap();
        assert_eq!(advice.root_cause, "认证失败");
        assert_eq!(advice.fix_suggestion, "重新登录");
    }

    #[tokio::test]
    async fn test_attribute_tolerates_missing_fix_suggestion() {
        // GAP-9：LLM 省略 fix_suggestion 字段不应导致整体失败
        let mut mock = MockClient::new();
        mock.expect_generate_text()
            .times(1)
            .returning(|_, _| Ok(r#"{"root_cause":"timeout"}"#.to_string()));

        let attributor = FailureAttributor::new(&mock);
        let advice = attributor.attribute(&sample_context()).await.unwrap();
        assert_eq!(advice.root_cause, "timeout");
        assert_eq!(advice.fix_suggestion, "");
    }

    #[tokio::test]
    async fn test_attribute_rejects_malformed_json() {
        let mut mock = MockClient::new();
        mock.expect_generate_text()
            .times(1)
            .returning(|_, _| Ok("Not a json object at all".to_string()));

        let attributor = FailureAttributor::new(&mock);
        let res = attributor.attribute(&sample_context()).await;
        assert!(matches!(res, Err(AiError::TranslationFailed(_))));
    }

    #[tokio::test]
    async fn test_attribute_rejects_empty_root_cause() {
        let mut mock = MockClient::new();
        mock.expect_generate_text()
            .times(1)
            .returning(|_, _| Ok(r#"{"root_cause":"   "}"#.to_string()));

        let attributor = FailureAttributor::new(&mock);
        let res = attributor.attribute(&sample_context()).await;
        assert!(matches!(res, Err(AiError::TranslationFailed(_))));
    }
}
