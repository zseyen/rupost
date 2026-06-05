# RuPost AI 接入实施方案 (方案 A：嵌入式集成)

## 1. 概述
本方案采用 **嵌入式集成 (Embedded Integration)** 模式，直接在 RuPost 核心通过 Rust 代码调用 AI Provider API。
目标是以最小的架构复杂度，提供智能补全、分析和调试功能。

## 2. 总体架构图

```mermaid
graph TD
    User[(用户 / CLI / TUI)] -->|调用| Core[RuPost Core Logic]
    
    subgraph "AI Services Layer (src/ai)"
        Core -->|Request| AiManager[AI Manager]
        
        AiManager -->|Config| ConfigLoader[Config Loader]
        AiManager -->|Prompt| PromptBuilder[Prompt Builder]
        
        AiManager -->|Trait Call| Provider{可以 AI Provider}
        
        Provider -->|Impl| OpenAI[OpenAI (async-openai)]
        Provider -->|Impl| Ollama[Ollama (Local)]
        Provider -->|Impl| SiliconFlow[SiliconFlow (DeepSeek)]
    end
    
    ConfigLoader -->|Read| ConfigFile[rupost.toml / .env]
    PromptBuilder -->|Load| Templates[Prompt Templates]
    
    Provider -->|HTTP/SSE| ExternalAPI[外部 AI API]
```

## 3. 详细设计

### 3.1 核心组件 (`src/ai`)

#### A. 配置管理 (`config`)
在 `rupost.toml` 中增加 `[ai]` 节：

```toml
[ai]
enable = true
default_provider = "deepseek"
timeout = 30 # seconds

[ai.providers.deepseek]
type = "openai_compatible"
api_base = "https://api.siliconflow.cn/v1"
api_key = "sk-..."
model = "deepseek-ai/DeepSeek-V3"

[ai.providers.ollama]
type = "ollama"
api_base = "http://localhost:11434"
model = "llama3"
```

#### B. Provider 抽象
定义 `AiProvider` Trait，屏蔽不同后端差异：

```rust
#[async_trait]
pub trait AiProvider {
    // 简单的问答
    async fn chat(&self, messages: Vec<Message>) -> Result<String>;
    
    // 流式输出 (用于 TUI 实时显示)
    async fn chat_stream(&self, messages: Vec<Message>, callback: Box<dyn Fn(String) + Send>);
}
```

#### C. Prompt 构建器 (`prompt`)
为了方便管理 prompt，不硬编码在代码中。使用 `minijinja` 模板引擎。

**模板示例 (`templates/analyze_error.j2`):**
```text
You are an expert API debugger.
Analyze this failed HTTP request:

Request: {{ method }} {{ url }}
Headers: {{ request_headers }}
Body: {{ request_body | truncate(500) }}

Response: {{ status_code }}
Body: {{ response_body | truncate(1000) }}

Please explain why it failed and suggest a fix.
```

### 3.2 功能模块实现

#### 功能 1: 智能分析 (Smart Analysis)
*   **入口**: TUI 历史记录界面 -> 按 `a`。
*   **逻辑**: 
    1. 获取当前选中的 `Exchange` (Req + Resp)。
    2. 自动截断超长 Body (超过 2000 字符截断，防止 Token 爆炸)。
    3. 调用 `AiProvider::chat_stream`。
    4. TUI 弹出一个 `Dialog`，实时追加显示 AI 返回的 markdown 文本。

#### 功能 2: 智能生成 (Text-to-Request)
*   **入口**: CLI `rupost ai gen "get user 123"` 或 TUI 输入框。
*   **逻辑**:
    1. Prompt: "Translate this natural language to a RuPost request object JSON..."
    2. 解析返回的 JSON，转换为 RuPost 内部 `Request` 对象。
    3. 填充到 TUI 编辑区。

## 4. 注意事项 (Precautions)

### 4.1 隐私与安全 (Privacy & Security)
> [!WARNING]
> **API Key 泄露风险**: 用户可能将含有 Key 的配置文件提交到 Git。
*   **对策**: 
    1. 强制要求 API Key 必须从环境变量 (`RUPOST_AI_KEY`) 读取，或存储在系统级安全位置 (`~/.config/rupost/secrets.toml`)，**严禁** 放入项目级 `rupost.toml` (该文件通常受版本控制)。
    2. 在 UI 中显示 Key 时通过 `******` 脱敏。
> **数据泄露风险**: 业务敏感数据发送给云端 AI。
*   **对策**: 
    1. 增加 **“敏感模式 (Redacted Mode)”** 开关。开启后，自动替换 Body 中的 email, phone, password 等字段为 `[REDACTED]`。
    2. 默认支持并推广 **Ollama (Local)** 模式，适合企业内网环境。

### 4.2 成本控制 (Cost Control)
*   **Token 消耗**: 大 JSON 响应可能瞬间消耗大量 Token。
*   **对策**:
    1. **硬性截断**: 默认只发送 Response Body 的前 1KB - 2KB。
    2. **Token 估算**: 在发送前简单估算 Token 数，超过阈值提示用户确认。

### 4.3 稳定性与错误处理
*   **API 超时**: AI 响应通常较慢 (5s - 30s)。
*   **对策**:
    1. 全程 **Async**，绝不阻塞 UI 线程。
    2. TUI 必须显示明显的 Loading Spinner。
    3. 支持 `Ctrl+C` 随时取消生成。

### 4.4 依赖管理
*   引入 `async-openai` 和 `tokio-stream` 会增加编译时间和二进制体积。
*   建议通过 `cargo` feature gate (`features = ["ai"]`) 管理，允许用户编译无 AI 版本的 RuPost。

## 5. 实施计划 (Task Breakdown)
1.  **基础层**: 引入 `reqwest`/`async-openai`，实现 `AiProvider` trait 和 DeepSeek/OpenAI 适配。
2.  **配置层**: 更新 `Config` 结构体，支持 AI 相关配置读取。
3.  **UI 层**: 改造 TUI，增加 `AnalysisDialog` 组件，支持流式渲染 Markdown。
4.  **逻辑层**: 实现“错误分析”的 Prompt 模板和组装逻辑。
