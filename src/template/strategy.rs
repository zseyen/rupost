pub trait TemplateStrategy {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn http_content(&self) -> &'static str;
    fn markdown_content(&self) -> &'static str;
    fn env_content(&self) -> Option<&'static str>;
}

pub struct SseTemplate;

impl TemplateStrategy for SseTemplate {
    fn name(&self) -> &'static str {
        "sse"
    }

    fn description(&self) -> &'static str {
        "Server-Sent Events & LLM Stream testing template"
    }

    fn http_content(&self) -> &'static str {
        include_str!("../../templates/sse/template.http")
    }

    fn markdown_content(&self) -> &'static str {
        include_str!("../../templates/sse/template.md")
    }

    fn env_content(&self) -> Option<&'static str> {
        Some(include_str!("../../templates/sse/env.example"))
    }
}
