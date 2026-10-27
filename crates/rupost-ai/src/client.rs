use crate::error::{AiError, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// AI 请求总超时：AI 端点响应慢，但绝不允许无限阻塞脚本与 CI 流水线
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[allow(clippy::double_must_use)]
#[async_trait]
pub trait AiClient: Send + Sync {
    /// Send a prompt to the AI and get a text response
    async fn generate_text(&self, system_prompt: &str, user_prompt: &str) -> Result<String>;
}

/// Simple OpenAI-compatible client
pub struct OpenAiClient {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
}

impl OpenAiClient {
    pub fn new(api_key: String, base_url: Option<String>, model: Option<String>) -> Self {
        Self {
            client: Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .build()
                .expect("failed to build HTTP client"),
            api_key,
            base_url: base_url.unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
            model: model.unwrap_or_else(|| "gpt-4o-mini".to_string()),
        }
    }
}

#[derive(Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: MessageContent,
}

#[derive(Deserialize)]
struct MessageContent {
    content: String,
}

#[async_trait]
impl AiClient for OpenAiClient {
    async fn generate_text(&self, system_prompt: &str, user_prompt: &str) -> Result<String> {
        let request = ChatRequest {
            model: self.model.clone(),
            messages: vec![
                Message {
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                },
                Message {
                    role: "user".to_string(),
                    content: user_prompt.to_string(),
                },
            ],
        };

        let response = self
            .client
            .post(format!(
                "{}/chat/completions",
                self.base_url.trim_end_matches('/')
            ))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(AiError::ApiError {
                status: status.as_u16(),
                message: error_text,
            });
        }

        let mut chat_response: ChatResponse = response.json().await?;
        if chat_response.choices.is_empty() {
            return Err(AiError::TranslationFailed(
                "No choices returned from AI".to_string(),
            ));
        }

        Ok(chat_response.choices.remove(0).message.content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_client(uri: String) -> OpenAiClient {
        OpenAiClient::new(
            "test-key".to_string(),
            Some(uri),
            Some("test-model".to_string()),
        )
    }

    #[tokio::test]
    async fn test_generate_text_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("Authorization", "Bearer test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": "hello assertion"}}]
            })))
            .mount(&server)
            .await;

        let client = test_client(server.uri());
        let result = client.generate_text("sys", "user").await.unwrap();
        assert_eq!(result, "hello assertion");
    }

    #[tokio::test]
    async fn test_generate_text_tolerates_trailing_slash_base_url() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": "ok"}}]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(format!("{}/", server.uri()));
        let result = client.generate_text("sys", "user").await.unwrap();
        assert_eq!(result, "ok");
        server.verify().await;
    }

    #[tokio::test]
    async fn test_generate_text_api_error_includes_status_and_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(401)
                    .set_body_string(r#"{"error": {"message": "invalid api key"}}"#),
            )
            .mount(&server)
            .await;

        let client = test_client(server.uri());
        let result = client.generate_text("sys", "user").await;
        match result {
            Err(AiError::ApiError { status, message }) => {
                assert_eq!(status, 401);
                assert!(message.contains("invalid api key"));
            }
            other => panic!("Expected ApiError, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_generate_text_empty_choices() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": []
            })))
            .mount(&server)
            .await;

        let client = test_client(server.uri());
        let result = client.generate_text("sys", "user").await;
        match result {
            Err(AiError::TranslationFailed(msg)) => assert!(msg.contains("No choices")),
            other => panic!("Expected TranslationFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_generate_text_non_json_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not a json"))
            .mount(&server)
            .await;

        let client = test_client(server.uri());
        let result = client.generate_text("sys", "user").await;
        assert!(
            matches!(result, Err(AiError::RequestFailed(_))),
            "Expected RequestFailed for non-JSON body, got: {result:?}"
        );
    }
}
