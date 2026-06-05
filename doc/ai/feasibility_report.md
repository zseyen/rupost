# RuPost AI 接入可行性分析报告

## 1. 概述
本报告旨在分析 RuPost 项目接入 AI 能力的可行性、技术方案及实施路线。目标是通过 AI 技术提升 API 调试、测试和文档生成的效率，实现“智能辅助”的开发体验。

## 2. 核心场景分析 (对应规划 12.1 - 12.7)

### 12.1 智能提示 (Smart Suggestions) & 12.2 智能补全 (Smart Completion)
*   **场景描述**: 在用户输入 URL、Header 或 Body 时，根据上下文自动提示。例如输入 `Content-Type:` 自动提示 `application/json`；或根据 URL 路径猜测可能需要的参数。
*   **技术可行性**: **高**。
    *   **方案 A (本地规则 + 统计)**: 不依赖 LLM，使用 Trie 树或频率统计进行补全（响应快，无隐私问题）。
    *   **方案 B (LLM 辅助)**: 对于复杂 Body 结构，可将 URL 和 Method 发送给 LLM 获取示例模版。
*   **推荐策略**: 优先实现 **方案 A**（本地补全），将 **方案 B** 作为“生成模版”的按需功能。

### 12.3 智能分析 (Smart Analysis)
*   **场景描述**: 对 API 响应（尤其是错误响应或大 JSON）进行分析。例如：“分析这个 500 错误的可能原因”、“总结这个 JSON 返回的关键数据字段”。
*   **技术可行性**: **极高**。这是 LLM 最擅长的领域。
*   **实现**: 将 Request + Response (截断后) 发送给 LLM，Prompt 如：“请分析此 API 响应，指出潜在问题或优化建议”。

### 12.4 智能调试 (Smart Debugging)
*   **场景描述**: 当请求失败或未达预期时，AI 给出修复建议。例如：“Curl 命令在终端能通，但在 RuPost 报错”，AI 分析差异（如 Header 缺失、Cookie 问题）。
*   **技术可行性**: **高**。
*   **实现**: 捕获错误上下文（RuPost 内部 Error、网络报错），结合 Request 配置，询问 LLM。

### 12.5 智能生成 (Smart Generation)
*   **场景描述**:
    1.  **自然语言转 Request**: 输入“查询最近 3 天的订单”，自动生成 GET `/orders?days=3`。
    2.  **Curl/文档转 Request**: 粘贴 Curl 或 Swagger 片段，自动转换为 RuPost 格式。
*   **技术可行性**: **中到高**。自然语言转换准确率依赖模型能力，但 Curl 转换可以通过确定性解析器实现（无需 LLM），复杂场景再用 LLM 兜底。

### 12.6 智能测试 (Smart Testing)
*   **场景描述**:
    1.  **自动生成断言**: 根据 Response 内容，自动生成 `@assert status == 200` 等断言代码。
    2.  **自动生成测试用例**: 根据 API 定义，生成边界测试（如参数为空、非法字符）。
*   **技术可行性**: **高**。
*   **价值**: **极高**。大幅降低编写测试脚本的门槛。

### 12.7 MCP (Model Context Protocol) 和 Skills
*   **场景描述**:
    1.  **作为 MCP Server**: 让 Claude/Cursor 等外部 AI 工具能直接调用 RuPost 发送请求（RuPost 变为 AI 的“网络手脚”）。
    2.  **扩展 Skills**: 允许用户编写自定义脚本（Skill），结合 AI 处理特定逻辑（如“自动获取 Token 并签名”）。
*   **技术可行性**: **中**。
    *   MCP 协议尚在发展中，Rust 生态已有基础支持（或需自研适配层）。
    *   这将极大地扩展 RuPost 的生态位，使其成为 Agentic Workflow 的一部分。

## 3. 技术架构方案

### A. 模型接入层 (Model Provider Layer)
为了兼顾隐私（企业用户）和便利性（个人用户），建议采用 **Provider 模式**：
*   **Trait 设计**: 定义 `AiProvider` trait (`complete`, `analyze`, `generate`)。
*   **支持后端**:
    *   **Cloud**: OpenAI, Anthropic, DeepSeek, SiliconFlow (硅基流动) 等兼容 OpenAI 格式的 API。
    *   **Local**: Ollama (调用本地 llama3/qwen 模型)，保障 0 数据泄露。

### B. 交互与配置
*   **配置**: 在 `rupost.toml` 或 `~/.rupost/config` 中配置 Provider 和 API Key。
*   **UI 交互**:
    *   CLI: `rupost ai generate "get user profile"`
    *   TUI: 专用快捷键（如 `Ctrl+A`）唤起 AI 面板，进行当前上下文的对话。

## 4. 实施路线图 (Roadmap)

### 第一阶段：基础设施与辅助 (MVP)
*   [ ] 引入 `async-openai` 或通用 HTTP Client 封装 AI 调用层。
*   [ ] 实现全局配置 (Provider, API Key)。
*   [ ] **功能 12.3 & 12.4**: 实现“分析当前请求/响应”功能。
*   [ ] **功能 12.6**: 实现“根据响应生成断言”功能。

### 第二阶段：生成与补全
*   [ ] **功能 12.5**: 实现“自然语言生成请求”。
*   [ ] **功能 12.1**: 探索基于 LLM 的智能补全（需解决延迟问题）。

### 第三阶段：生态集成 (MCP)
*   [ ] **功能 12.7**: 实现 MCP Server 接口，允许外部 Agent 调用 RuPost 执行 API 测试。

## 5. 风险与对策
| 风险点 | 描述 | 对策 |
| :--- | :--- | :--- |
| **隐私泄露** | 用户不希望将内部 API 数据发送给云端 AI | 1. 强推 Ollama 本地模型支持。<br>2. 增加“敏感字段脱敏”预处理。<br>3. 默认关闭自动发送，需用户手动确认。 |
| **响应延迟** | AI 调用耗时较长，影响 CLI/TUI 流畅度 | 1. 异步处理 AI 请求，UI 显示 Loading 状态但不阻塞主线程。<br>2. 补全类功能设置超时熔断。 |
| **Token 消耗** | 长 JSON 响应导致 Token 费用过高 | 1. 自动截断超长 Response。<br>2. 仅提取关键结构发送给 AI。 |

## 6. 结论
在 RuPost 中接入 AI 是**高可行性且高价值**的。
推荐优先从 **“辅助测试” (智能断言)** 和 **“辅助分析” (错误诊断)** 入手，这两个场景对实时性要求不高，但能显著提升用户体验。技术上首选兼容 OpenAI 接口的 SDK，并同步支持 Ollama 以满足隐私需求。
