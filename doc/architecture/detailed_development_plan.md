# RuPost 详细开发计划与测试方案 (Detailed Development Plan)

## 1. 概述
本文档将“总体架构方案”转化为可执行的代码开发任务 (WBS)。
遵循 **"Cookie (P0) -> AI (P1) -> Script (P2) -> Doc (P3)"** 的优先级。

---

## 2. Phase 1: 核心基石 (Cookies & AI Basic)
**目标**: 完成 HTTP 状态管理，并上线第一版 AI 智能分析。

### 2.1 功能规格 (Specs)

#### Feature A: Cookie Middleware
*   **依赖**: `reqwest_cookie_store`, `cookie_store`.
*   **逻辑**:
    *   启动时从 `rupost.toml` 或 CLI 参数 `--cookie-jar`读取路径（默认 `.rupost/cookies.json`）。
    *   构建 `Arc<CookieStore>`。
    *   **Hook**: 在 `execute_request` 中，Reqwest 会自动处理 Cookie，但我们需要确保 Store 的持久化。
    *   **Persist**: 每次 Request 结束后，若 Store 发生变化（Dirty check），加文件锁写入磁盘。

#### Feature B: AI Analysis Service
*   **依赖**: `async-openai`, `minijinja`.
*   **逻辑**:
    *   新增 `src/services/ai.rs`。
    *   实现 `AiProvider` trait (DeepSeek/OpenAI)。
    *   Prompt: 提取 Request (URL, Method, Headers) + Response (Status, Body Snippet)，组装成 Markdown Prompt。
    *   TUI: 监听 `Key::Char('a')`，触发异步任务，弹窗显示 Loading -> Markdown Stream。

### 2.2 详细测试用例 (Test Cases)

#### [Test-Cookie-01] 302 重定向连续性 (错误高发点)
*   **场景**: 登录接口返回 302 并 Set-Cookie，重定向后的 GET 请求必须携带该 Cookie。
*   **代码验证**:
    ```rust
    // Mock Server 模拟
    let mock = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(302).append_header("Set-Cookie", "sid=123").append_header("Location", "/profile")).mount(&mock);
    Mock::given(method("GET")).match_header("Cookie", "sid=123").respond_with(ResponseTemplate::new(200)).mount(&mock);
    // 执行 Request
    client.execute(...).await?; 
    // Assert: 第二个请求成功返回 200
    ```

#### [Test-Cookie-02] 域名隔离 (错误高发点)
*   **场景**: Cookie 设置为 domain=`api.example.com`，请求 `hack.example.com` 时不应携带。
*   **验证**: 构造两个不同域名的 Request，验证 Store 的 `matches()` 逻辑。

#### [Test-AI-01] Body 截断保护
*   **场景**: Response Body 是 10MB 的日志文件。
*   **验证**: 确保发送给 LLM 的 Prompt 中，Body 被截断为前 2KB，且包含 `...[truncated]` 标记。避免 Token 溢出错误。

---

## 3. Phase 2: 深度能力 (Scripting & Prod Debug)
**目标**: 支持复杂签名计算和生产环境 Debug。

### 3.1 功能规格 (Specs)

#### Feature C: Rhai Scripting
*   **依赖**: `rhai`.
*   **逻辑**:
    *   `src/scripting/engine.rs`: 初始化 Engine，注册 `md5`, `sha256`, `time` 等函数。
    *   **Scope**: 将 Rust `Request` 转换为 `Map` 传入 Rhai，执行后再读回。
    *   **Hook**: `Pre-request` (修改 Header/Query), `Post-response` (Assert, Env Set).

#### Feature D: Production Replay
*   **逻辑**:
    *   `src/importers/log_parser.rs`: 解析 Nginx/JSON 日志。
    *   将日志行转换为 `Request` 对象列表。
    *   利用 `Runner` 批量执行。

### 3.2 详细测试用例 (Test Cases)

#### [Test-Script-01] 死循环熔断 (关键安全点)
*   **场景**: 用户脚本 `while true {}`。
*   **验证**: 设定 Rhai 引擎 `on_progress` 回调，超过 10,000 operations 强制抛错 `Error: Script timeout`。

#### [Test-Script-02] 变量作用域污染
*   **场景**: 脚本 A 设置了 `global.token`，脚本 B 应该能读取；但脚本 A 定义的 `let local_var` 不应泄露给 B。
*   **验证**: 运行两个连续脚本，检查变量可见性。

---

## 4. Phase 3: 生态扩展 (Doc Gen)
**目标**: 实现 SSOT 文档生成。

### 4.1 功能规格 (Specs)
*   **逻辑**:
    *   增强 Parser，识别 `/// @desc` 和 `/// @param`。
    *   使用 `utoipa-gen` 或手动构建 `OpenAPI` JSON 结构。
    *   嵌入 `Redoc` 的 HTML 模板字符串。

### 4.2 详细测试用例 (Test Cases)

#### [Test-Doc-01] 敏感信息过滤 (风险点)
*   **场景**: Request Header 中包含 `Authorization: Bearer my-secret-token`。
*   **验证**: 生成的 `openapi.json` 中，该 Header 的 `example` 字段应为空或 `******`，确保秘密不泄露进文档。

#### [Test-Doc-02] 类型推导
*   **场景**: Body 为 `{"ids": [1, 2, "3"]}` (混合类型)。
*   **验证**: 生成的 Schema 应为 `oneOf: [integer, string]` 或退化为 `string`，而不是报错。

---

## 5. 开发排期 (Timeline)

*   **Sprint 1 (Week 1)**: 
    *   Setup `src/middleware`, `src/services`.
    *   Implement **Feature A (Cookie)** & Unit Tests.
*   **Sprint 2 (Week 2)**:
    *   Implement **Feature B (AI Analysis)**.
    *   Upgrade TUI (Async Popup).
*   **Sprint 3 (Week 3)**:
    *   Release v0.5.0 (Beta).
    *   Start **Feature C (Scripting)** POC.

---

## 6. 规划自审与修正建议 (Self-Assessment & Revisions)

### 6.1 ✅ 规划中合理的部分

| 维度 | 决策 | 评价 |
| :--- | :--- | :--- |
| **优先级排序** | P0 Cookie -> P1 AI -> P2 Script | **正确**。Cookie 是基础设施，AI 是差异化卖点。先生存，再发展。 |
| **技术选型** | Rhai (脚本), Reqwest (HTTP), Async-OpenAI (AI) | **合理**。全 Rust 生态，编译体积可控，与现有 Tokio 运行时兼容。 |
| **架构模式** | Middleware 链式处理 | **优秀**。符合开闭原则，新增功能无需修改 Core。 |
| **测试设计** | 针对 302 跳转、死循环、Body 截断等场景 | **到位**。抓住了真正容易出错的关键路径。 |

### 6.2 ⚠️ 潜在风险与待商榷点

#### 风险 1: TUI 的隐性复杂度被低估 (P1.5)
*   **问题**: 规划中 TUI 只是"跟随升级"，但实际上"异步弹窗 + 流式 Markdown 渲染"本身就是一个高难度任务。Ratatui 生态对此支持有限，可能需要自己造轮子。
*   **建议**: 将 TUI 从 P1.5 提升到与 AI **并行开发**。如果 AI 功能开发完成但 TUI 无法承载，会导致功能"可用但不可见"。可以考虑先实现"CLI Only"的 AI 分析版本作为降级方案。

#### 风险 2: Cookie 持久化与多环境冲突
*   **问题**: 规划中提到"多环境隔离"（`cookies_dev.json`, `cookies_prod.json`），但如果用户在同一个终端内频繁切换环境，可能出现 Cookie 混用。
*   **建议**: 在 Cookie 存储层增加 `environment_id` 的 Key 隔离，而非仅靠文件名区分。

#### 风险 3: AI 功能依赖外部网络
*   **问题**: Phase 1 的 AI 功能完全依赖云端 LLM。如果用户网络环境差（如中国大陆直连 OpenAI），体验会很糟糕。
*   **建议**: 在规划中**强化 Ollama (本地) 支持的优先级**。即使是 Demo 阶段，也提供 "Fallback to Local" 的选项，让用户在内网/离线环境也能体验。

#### 风险 4: 测试用例粒度不够细
*   **问题**: 当前测试用例偏向"功能层面"，但缺少"集成层面"的边界 Case。例如：
    *   如果 Cookie Middleware 和 Script Middleware **同时存在**，执行顺序是否正确？
    *   如果 AI 请求**正在流式返回时**用户按下 `Esc` 取消，资源是否能正确释放？
*   **建议**: 在 Phase 2 开始前，补充一组 **Integration Test** 用例，专门测试 Middleware 链的组合行为。

#### 风险 5: 排期过于乐观
*   **问题**: 规划中 Sprint 1 (1 周) 要完成 Cookie 全部功能 + Unit Tests，Sprint 2 (1 周) 要完成 AI + TUI 升级。对于单人开发者，这个节奏非常紧张。
*   **建议**: 将 Phase 1 拆分为两个 Release：
    *   `v0.5.0-alpha`: 仅 Cookie (无 UI)，验证核心逻辑。
    *   `v0.5.0-beta`: 增加 AI + TUI。

### 6.3 🎯 修正后的排期 (Revised Timeline)

*   **Sprint 1 (Week 1-1.5)**:
    *   Setup `src/middleware`, `src/services`.
    *   Implement **Feature A (Cookie)** & Unit Tests.
    *   **Release v0.5.0-alpha** (验证 Cookie 核心逻辑)。
*   **Sprint 2 (Week 2-2.5)**:
    *   Implement **Feature B (AI Analysis)** with **CLI-Only** mode first.
    *   Parallel: Upgrade TUI (Async Popup POC).
    *   **Release v0.5.0-beta**。
*   **Sprint 3 (Week 3-4)**:
    *   Stabilize TUI + AI integration.
    *   Start **Feature C (Scripting)** POC.
    *   **Release v0.5.0** (正式版)。

### 6.4 待补充的集成测试用例 (Integration Tests)

#### [Test-Integration-01] Middleware 链顺序
*   **场景**: Cookie + Script 同时启用。
*   **验证**: Pre-Script 应在 Cookie Inject **之前**执行（允许脚本覆盖 Cookie Header），Post-Script 应在 Cookie Extract **之后**执行。

#### [Test-Integration-02] AI 取消安全
*   **场景**: AI 正在流式返回，用户按 `Esc`。
*   **验证**: 检查 `tokio::spawn` 任务是否被 `abort()`，且无内存泄漏（使用 `valgrind` 或 `tokio-console`）。

