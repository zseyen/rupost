---
name: ai-integration-guidelines
description: "Phased AI strategy for Rupost, covering Embedded integration, MCP server architecture, and Prompt engineering."
---

# AI Integration & Evolution Strategy

## 1. Phased Roadmap
Follow the established technical roadmap to balance immediate value with future scalability.

- **Phase 1: Embedded Integration (Current Focus)**
    - Integration of lightweight AI clients (e.g., `async-openai`) directly into the binary.
    - Zero-config requirement for users (mostly via Environment Variables).
- **Phase 2: MCP-First Evolution**
    - Expose Rupost's core capabilities (executing requests, reading history) as an **MCP Server**.
    - This allows external Agents (Claude, Cursor) to control Rupost.

## 2. Implementation Standards

### Provider Abstraction
AI functionality MUST be decoupled via the `AiProvider` trait to support multiple models (OpenAI, DeepSeek, Local Ollama).

```rust
#[async_trait]
pub trait AiProvider: Send + Sync {
    async fn complete(&self, prompt: &str) -> Result<String>;
    async fn stream_complete(&self, prompt: &str, tx: mpsc::Sender<String>) -> Result<()>;
}
```

### Prompt Engineering
- **Templates**: Never hardcode complex prompts. Use `minijinja` or `tera` templates.
- **Context Management**: Be conservative with context. Truncate long response bodies before sending to LLM.
- **Markdown Friendly**: All AI generations/analyses should follow Markdown for better rendering in the TUI/CLI.

### User Privacy & Control
- **Explicit Opt-in**: AI features should not send data without clear user intent (e.g., pressing a specific key).
- **Redaction**: Automatically redact sensitive headers (e.g., `Authorization`, `Cookie`) from prompts sent to third-party APIs.

## 3. Core AI Use Cases
- **Smart Analysis**: Explain failure reasons based on Request/Response context.
- **Test Generation**: Convert natural language descriptions into valid `.http` blocks.
- **Assertion Suggestion**: Suggest `@assert` lines based on a successful response structure.
