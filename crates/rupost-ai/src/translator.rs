use crate::client::AiClient;
use crate::error::{AiError, Result};
use std::sync::Arc;

/// Translates natural language assertions into standard RuPost assertions
pub struct AssertionTranslator {
    client: Arc<dyn AiClient>,
}

impl AssertionTranslator {
    pub fn new(client: Arc<dyn AiClient>) -> Self {
        Self { client }
    }

    /// Translate a natural language assertion into a list of standard assertions
    pub async fn translate(&self, natural_language: &str) -> Result<Vec<String>> {
        let system_prompt = "You are an API testing assistant for RuPost. \
Your task is to translate natural language expectations into strict RuPost assertion expressions. \
RuPost assertion expressions must follow this exact format: \
<path> <operator> <value>. \
Supported paths: status, headers.<name>, body.<json_path>, response.time. \
Supported operators: ==, !=, >, <, >=, <=, contains, exists. \
Return ONLY a JSON array of strings, where each string is a standard assertion. \
Do not return any markdown formatting or explanations. Just the JSON array. \
Example input: 'Response is 200 and token exists' \
Example output: [\"status == 200\", \"body.token exists\"]";

        let response = self.client.generate_text(system_prompt, natural_language).await?;
        
        let mut cleaned = response.trim();
        if cleaned.starts_with("```json") {
            cleaned = cleaned.strip_prefix("```json").unwrap();
        } else if cleaned.starts_with("```") {
            cleaned = cleaned.strip_prefix("```").unwrap();
        }
        if cleaned.ends_with("```") {
            cleaned = cleaned.strip_suffix("```").unwrap();
        }
        cleaned = cleaned.trim();

        let parsed: Vec<String> = serde_json::from_str(cleaned).map_err(|e| {
            AiError::TranslationFailed(format!("Failed to parse AI output as JSON array. Error: {}. Output was: {}", e, response))
        })?;

        Ok(parsed)
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

    #[tokio::test]
    async fn test_translate_success() {
        let mut mock_client = MockClient::new();
        mock_client
            .expect_generate_text()
            .withf(|_sys, user| user == "登录成功并且包含token")
            .times(1)
            .returning(|_, _| Ok(r#"["status == 200", "body.token exists"]"#.to_string()));

        let translator = AssertionTranslator::new(Arc::new(mock_client));
        let assertions = translator.translate("登录成功并且包含token").await.unwrap();

        assert_eq!(assertions.len(), 2);
        assert_eq!(assertions[0], "status == 200");
        assert_eq!(assertions[1], "body.token exists");
    }

    #[tokio::test]
    async fn test_translate_with_markdown_wrapper() {
        let mut mock_client = MockClient::new();
        mock_client
            .expect_generate_text()
            .times(1)
            .returning(|_, _| Ok("```json\n[\"status == 200\"]\n```".to_string()));

        let translator = AssertionTranslator::new(Arc::new(mock_client));
        let assertions = translator.translate("200").await.unwrap();

        assert_eq!(assertions.len(), 1);
        assert_eq!(assertions[0], "status == 200");
    }

    #[tokio::test]
    async fn test_translate_invalid_json() {
        let mut mock_client = MockClient::new();
        mock_client
            .expect_generate_text()
            .times(1)
            .returning(|_, _| Ok("I think it should be status == 200".to_string()));

        let translator = AssertionTranslator::new(Arc::new(mock_client));
        let result = translator.translate("200").await;

        assert!(result.is_err());
        match result {
            Err(AiError::TranslationFailed(msg)) => {
                assert!(msg.contains("Failed to parse AI output"));
            }
            _ => panic!("Expected TranslationFailed error"),
        }
    }
}
