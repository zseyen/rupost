use reqwest::header::HeaderMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmProvider {
    OpenAi,
    Anthropic,
    Generic,
}

pub struct LlmStreamAdapter;

impl LlmStreamAdapter {
    pub fn detect_provider(_url: &str, _headers: &HeaderMap) -> LlmProvider {
        // MVP 骨架暂时返回 Generic
        LlmProvider::Generic
    }

    pub fn extract_delta(&self, _provider: LlmProvider, _data: &str) -> Option<String> {
        // MVP 骨架暂时返回 None
        None
    }
}
