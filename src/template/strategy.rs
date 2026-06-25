pub trait TemplateStrategy {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn http_content(&self) -> &'static str;
    fn markdown_content(&self) -> &'static str;
    fn env_content(&self) -> Option<&'static str>;
}

pub struct ConfigTemplate;

impl TemplateStrategy for ConfigTemplate {
    fn name(&self) -> &'static str {
        "config"
    }

    fn description(&self) -> &'static str {
        "Initialize default rupost.toml configuration"
    }

    fn http_content(&self) -> &'static str {
        include_str!("../../templates/config/rupost.toml")
    }

    fn markdown_content(&self) -> &'static str {
        include_str!("../../templates/config/rupost.toml")
    }

    fn env_content(&self) -> Option<&'static str> {
        None
    }
}

pub struct SseTemplate;

impl TemplateStrategy for SseTemplate {
    fn name(&self) -> &'static str {
        "sse"
    }

    fn description(&self) -> &'static str {
        "Server-Sent Events generic testing template"
    }

    fn http_content(&self) -> &'static str {
        include_str!("../../templates/sse/template.http")
    }

    fn markdown_content(&self) -> &'static str {
        include_str!("../../templates/sse/template.md")
    }

    fn env_content(&self) -> Option<&'static str> {
        None
    }
}

pub struct LlmTemplate;

impl TemplateStrategy for LlmTemplate {
    fn name(&self) -> &'static str {
        "llm"
    }

    fn description(&self) -> &'static str {
        "Large Language Model (LLM) stream testing template"
    }

    fn http_content(&self) -> &'static str {
        include_str!("../../templates/llm/template.http")
    }

    fn markdown_content(&self) -> &'static str {
        include_str!("../../templates/llm/template.md")
    }

    fn env_content(&self) -> Option<&'static str> {
        Some(include_str!("../../templates/llm/env.example"))
    }
}
