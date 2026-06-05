use reqwest::header::HeaderMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmProvider {
    OpenAi,
    Anthropic,
    Generic,
}

pub struct LlmStreamAdapter;

impl LlmStreamAdapter {
    pub fn detect_provider(url: &str, _headers: &HeaderMap) -> LlmProvider {
        if url.contains("api.anthropic.com") || url.contains("/v1/messages") {
            LlmProvider::Anthropic
        } else if url.contains("api.openai.com")
            || url.contains("api.deepseek.com")
            || url.contains("/v1/chat/completions")
        {
            LlmProvider::OpenAi
        } else {
            LlmProvider::Generic
        }
    }

    pub fn extract_delta(&self, provider: LlmProvider, data: &str) -> Option<String> {
        match provider {
            LlmProvider::OpenAi => {
                let json: serde_json::Value = serde_json::from_str(data).ok()?;
                json.pointer("/choices/0/delta/content")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            }
            LlmProvider::Anthropic => {
                let json: serde_json::Value = serde_json::from_str(data).ok()?;
                json.pointer("/delta/text")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            }
            LlmProvider::Generic => {
                if data == "[DONE]" {
                    return None;
                }
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                    if let Some(c) = json.get("content").and_then(|v| v.as_str()) {
                        Some(c.to_string())
                    } else if let Some(t) = json.get("text").and_then(|v| v.as_str()) {
                        Some(t.to_string())
                    } else {
                        Some(data.to_string())
                    }
                } else {
                    Some(data.to_string())
                }
            }
        }
    }
}
