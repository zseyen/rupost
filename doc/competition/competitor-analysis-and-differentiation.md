# RuPost 竞品分析与差异化发展方案

## 1. 市场现状与竞品坐标

在当前的 API 调试与测试领域，市场已经趋于成熟，但仍存在明显的“体验断层”。

| 维度 | 传统 GUI (Postman/Apidog) | 现代本地化 (Bruno/Insomnia) | 纯 CLI (Curl/HTTPie) | 录制派 (Keploy) | 脚本派 (Hurl) | **RuPost (AntiGravity)** |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **形态** | 庞大的 Electron 应用 | 轻量级本地客户端 | 终端命令 | 流量代理/eBPF 代理 | 纯文本脚本执行器 | **高性能 Rust 终端工具** |
| **存储** | 云端同步（强制/半强制） | 本地 Git 友好文件 | 无持久化/脚本 | 自动生成的 YAML | .hurl 文件 | **Markdown/HTTP 文件 (100% Git 友好)** |
| **上手难度** | 低（可视化操作） | 中（需理解本地文件管理） | 高（参数复杂） | 极低（零代码录制） | 中（需学习自定义 DSL） | **极低（类 HTTP 语法 + 别名）** |
| **自动化支持** | 强大（脚本/断言） | 一般（断言支持中） | 弱（需配合 Shell） | 自动生成集成测试 | 强大（内置断言/链式请求） | **极强（原生 @assert/@capture）** |
| **核心痛点** | 臃肿、隐私风险、启动慢 | 缺乏深度的 CLI 交互 | 无法处理复杂逻辑 | 测试数据膨胀、环境依赖重 | 学习成本、文档化程度低 | **-** |

---

## 2. 差异化核心价值 (Core Differentiation)

RuPost 的差异化不应仅仅是“又一个测试工具”，而应是 **“开发者最亲近的 API 陪伴者”**。其核心差异化点在于：

### A. 文档即代码的终极形态 (Executable Documentation) vs Hurl
*   **Hurl**: 虽然也是文本驱动，但使用自定义的 `.hurl` 格式，主要面向测试工程师，作为“测试脚本”。
*   **RuPost 方案**: 深度解析 Markdown。
*   **差异化**: RuPost 将 Markdown 视为第一等公民。对于开发者来说，编写 `README.md` 中的 API 示例是天然的需求。RuPost 让这些示例“活”了过来，实现了**文档、示例、测试**的三位一体，而 Hurl 仍然是独立的测试文件。

### B. AI 原生驱动 vs Keploy
*   **Keploy**: 侧重于“无感录制”，通过流量回放生成测试。
*   **RuPost 方案**: 侧重于“意图驱动”。
*   **差异化**: Keploy 虽然高效但生成的测试用例往往是黑盒，难以阅读。RuPost 通过 AI 集成（如 `@ai_assert`），让开发者用自然语言定义测试逻辑。同时，RuPost 可以引入 **“录制到 Markdown”** 功能，将流量转化为易读、美观、可维护的 Markdown 文档。

### C. 极简主义的美学体验 (Developer Aesthetic)
*   **现状**: 终端输出冷冰冰，缺乏视觉引导（Hurl 和 Curl 都是此类）。
*   **RuPost 方案**: 采用 HSL 动态配色、微动画、以及沉浸式的 TUI (Terminal UI)。
*   **差异化**: 让测试变得愉悦。类似 `tig` 的交互体验，让历史记录回溯、请求修改、断言调试在毫秒间完成。

---

## 3. 差异化功能点设计 (Proposed Features)

### 1. **"Doc-Mock"：文档驱动的即时 Mock 服务器**
*   **描述**: 直接基于 Markdown 文件中的 HTTP 请求响应示例，启动一个本地 Mock Server。
*   **场景**: 前端开发者只需拿到后端的 Markdown 设计稿，运行 `rupost mock design.md`，即可开始联调。
*   **差异化**: 零配置，真正的单源真相（Single Source of Truth）。

### 2. **"Time-Travel"：交互式历史回溯与演进**
*   **描述**: 一个高性能的 TUI 仪表盘（`rupost ui`）。
*   **功能**:
    *   可视化对比两次请求的差异（Headers/Body/Timing）。
    *   一键将某次成功的历史请求“持久化”为 `.http` 文件中的测试用例。
    *   支持“请求录制”模式。
*   **差异化**: 将 CLI 的快与 GUI 的直观完美结合。

### 3. **"Stress-MD"：Markdown 里的压力测试**
*   **描述**: 在 `.http` 块中使用 `@bench` 指令，指定并发数和持续时间。
*   **功能**: 利用 Rust 协程进行高并发压测，并在终端生成实时波形图表。
*   **差异化**: 无需 Jmeter/Locust，在测试业务逻辑的同时顺便测试性能。

### 4. **"Smart-Capture"：上下文感知的变量自动关联**
*   **描述**: 自动识别登录接口返回的 Token，并在后续请求中智能建议注入。
*   **功能**: 减少手动 `@capture` 的工作量，实现“流式测试”。

---

## 4. MVP 路线图建议

1.  **Phase 1 (增强内核)**: 完善 `.md` 解析能力，支持多代码块上下文共享，强化 `@assert`。
2.  **Phase 2 (沉浸交互)**: 开发 `rupost h i` (Interactive History) TUI 界面，支持基础的编辑与重发。
3.  **Phase 3 (AI 协同)**: 集成 MCP (Model Context Protocol) 或内置 LLM 调用接口，实现 `@ai_assert`。
4.  **Phase 4 (生态扩展)**: 实现从 Markdown 自动启动 Mock Server 和性能压测。
