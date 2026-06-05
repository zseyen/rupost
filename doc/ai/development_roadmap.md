# RuPost AI 开发计划与架构演进文档

## 1. 开发阶段规划 (Development Roadmap)

采用敏捷迭代方式，分三个阶段落地。

### 阶段一：基础设施建设 (Infrastructure)
**目标**: 完成底层能力构建，实现配置读取和最基础的 AI 调用通路。

*   **Task 1.1: 引入依赖**
    *   在 `Cargo.toml` 添加 `async-openai`, `minijinja`, `tokio-stream`。
    *   创建独立 feature `ai`，确保不影响核心体积。
*   **Task 1.2: 配置系统升级**
    *   修改 `src/config.rs`，增加 `AiConfig` 结构体。
    *   实现从环境变量 `RUPOST_AI_KEY` 和配置文件 `rupost.toml` 读取配置的优先级逻辑。
*   **Task 1.3: 原型验证 (PoC)**
    *   创建 `src/ai/mod.rs`。
    *   实现 `AiProvider` trait 的基础版本。
    *   编写一个简单的 test case，验证能调通 DeepSeek/OpenAI 接口。

### 阶段二：核心功能实现 (Core Implementation)
**目标**: 落地“智能分析”功能，这是最高频场景。

*   **Task 2.1: Prompt 引擎**
    *   集成 `minijinja`。
    *   创建 `templates/analysis.j2` 模板。
    *   实现 Context 组装逻辑：提取 Request/Response 关键头和截断后的 Body。
*   **Task 2.2: TUI 交互改造**
    *   修改 `HistoryUI`，增加按键映射 `a` (Analyze)。
    *   **关键点**: 实现一个通用的 `StreamingDialog` 组件，支持异步接收 `mpsc::Receiver<String>` 并实时刷新 markdown 内容。
*   **Task 2.3: 错误处理与重试**
    *   处理网络超时、Token 超限 (429) 等常见错误。
    *   在 TUI 中优雅展示错误信息。

### 阶段三：拓展与优化 (Optimization)
**目标**: 落地“智能生成”功能，并进行性能优化。

*   **Task 3.1: 智能生成 (Text-to-Request)**
    *   在 CLI 实现子命令 `rupost ai gen <PROMPT>`。
    *   定义严格的 JSON Schema 输出，确保生成的 Request 对象可反序列化。
*   **Task 3.2: 隐私模式**
    *   实现 Body 过滤器 `Redactor`，用正则替换敏感字段。
    *   在配置中通过 `ai.redact_fields = ["password", "token"]` 控制。

---

## 2. 关联功能修改清单 (Impact Analysis)

为了支持 AI 接入，现有系统的以下模块需要配合修改：

| 模块 | 文件 (预测) | 修改内容 | 原因 |
| :--- | :--- | :--- | :--- |
| **Config** | `src/config.rs` | 增加 `[ai]` 字段解析；支持从 Env 读取 Secrets | 需要存储 API Key 和 Model 配置 |
| **TUI** | `src/tui/app.rs`<br>`src/tui/ui.rs` | 增加 Global Event Loop 对 AI 消息的处理；<br>新增 `AiDialog` 组件 | TUI 是单线程 Event Loop，AI 请求是异步的，需要跨线程通信 |
| **CLI** | `src/cli.rs` | 增加 `ai` 子命令 | 支持命令行交互 |
| **Core** | `src/runner.rs` | 暴露 Header/Body 的可截断 Debug 视图 | 如果现有 `Display` trait 输出太长或包含乱码，AI 无法处理 |
| **Meta** | `src/metadata.rs` | 预留 `@ai_ignore` 指令 | 允许用户在 `.http` 文件中标记某些请求“禁止 AI 分析” |

---

## 3. 架构演进与预留 (Future Proofing)

为了后续平滑升级到 MCP 架构或支持插件化，我们在设计方案 A 时，必须遵循 **依赖倒置 (DIP)** 和 **接口隔离 (ISP)** 原则。

### 3.1 预留 MCP 接口 (Adapter Pattern)
虽然现在是直接调用，但我们应该定义一组标准接口，这组接口未来可以直接映射为 MCP 的 Tool。

**Interface Definition (`src/ai/interface.rs`):**

```rust
// 这个结构体未来可以直接作为 MCP Server 的 Input Schema
pub struct AnalysisContext {
    pub method: String,
    pub url: String,
    pub status: u16,
    pub response_snippet: String,
}

// 业务逻辑层 (Business Logic Layer)
// 现在: 直接调用 LLM
// 未来: 这个函数可以注册为 MCP Tool 供外部调用
pub async fn analyze_request(ctx: AnalysisContext) -> Result<String> { ... }
```

### 3.2 预留插件化钩子 (Hook System)
不要把 AI 逻辑写死在 UI 层。

**Bad Design:**
```rust
// TUI 中
if key == 'a' {
    openai.chat(...) // 耦合了具体实现
}
```

**Good Design (Event Bus):**
```rust
// TUI 中
if key == 'a' {
    app.dispatch(AiEvent::RequestAnalysis(current_exchange_id));
}

// Event Handler
match event {
    AiEvent::RequestAnalysis(id) => {
        // 这里可以分发给内置 OpenAI，也可以分发给外部 Plugin
        ai_service.handle_analysis(id).await; 
    }
}
```

### 3.3 数据结构兼容性
确保 AI 生成的 `Request` JSON 结构与 RuPost 内部的 `Request` 结构体保持序列化兼容（serde 兼容）。这样未来如果通过 MCP 接收外部 Agent 生成的请求，也能直接反序列化执行。

## 4. 总结
本计划优先保证 **低耦合** 和 **非侵入式** 修改。核心改动集中在 `src/ai/` 目录下，对现有核心逻辑的修改尽量限制在“配置读取”和“事件分发”层面。
