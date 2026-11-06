# RuPost 全量后续规划用户故事（User Story）与系统架构走查报告

本报告扩展并建立了 Rupost 的完整用户画像（User Personas），推导出 12 个覆盖完整生命周期的 User Story，并对照当前系统底层的数据结构与 API 进行全方位的深度走查与 Gap 分析，最后针对核心问题（包括大模型流式断言、请求级拓扑依赖、以及网络诊断指标共用）提出具体的系统架构演进修改建议。

---

## 1. 深度扩展的用户画像 (User Personas)

### 👤 AI/LLM 全栈开发者 - 小明 (Ming) — 智能应用与长连接联调
*   **痛点**：调试 SSE 长连接及 Token 流时没有实时渲染反馈；API 密钥需要安全隔离；需要在同一文件内测试 Agent 的多轮对话依赖（从前一次大模型流中提取变量用于下一次请求）。
*   **关联规划**：SSE流处理 (WBS 12.0)、大模型模板与中转 (WBS 9.0)、变量捕获 (WBS 7.0)。

### 👤 后端研发工程师 - 小李 (Li) — 核心接口与性能保障
*   **痛点**：日常接口联调烦琐；复杂的微服务时序难以追溯；面临高并发或限流的生产问题时，缺乏排障日志和重现请求的手段。
*   **关联规划**：生产调试模式 (3)、WebSocket/MQTT (4)、高级断言 (7)、自动化与性能测试 (16, 17)。

### 👤 前端研发工程师 - 小张 (Zhang) — 联调协作与快速 mock
*   **痛点**：接口文档经常发生静默变更导致前端报错；缺少轻量级的 Mock API 独立验证逻辑；长连接及 GraphQL 的调试门槛高。
*   **关联规划**：导入/导出 (2)、WebSocket/GraphQL (4, 13)、API设计与 Mock (9)、TUI界面与主题 (10, 20)。

### 👤 测试自动化与安全工程师 - 小赵 (Zhao) — 质量与 CI/CD 守护者
*   **痛点**：庞大的测试套件之间缺乏依赖拓扑（Dependency Graph）编排，导致登录失效后全量用例挂掉；缺乏安全扫查（注入等）和数据脱敏。
*   **关联规划**：文件夹并行/串行测试 (5)、高级断言与流程控制 (7)、CI/CD 自动化 (8)、安全测试与自动化测试 (16, 18)。

### 👤 企业安全合规官 - 老孙 (Sun) — 插件安全与合规审计
*   **痛点**：接口调试工具缺乏审计日志，核心数据易泄漏；非授权的第三方插件可能引入代码注入风险。
*   **关联规划**：插件系统与插件安全 (1)、版本管理与多工作区 (11)、企业审计与合规 (14)、数据脱敏 (15)。

---

## 2. 覆盖完整生命周期的 12 个 User Story

### 📖 US 1: 文件夹测试的依赖拓扑与执行流编排 (WBS 5.0, 7.0, 16.0)
*   **场景**：作为测试开发，我希望可以一键按顺序或并行运行测试文件夹下的多个测试用例，且支持用例之间的依赖（如 `a.http` 执行成功后才执行 `b.md`）、条件控制（If/Else）和循环执行，以覆盖完整的复杂业务流断言。

### 📖 US 2: 同文件内“请求级”依赖与变量传递 (WBS 7.0)
*   **场景**：在大模型 Agent 开发中，我需要在同一个 HTTP/MD 文件中串行调试多个请求（例如：请求 1 登录获取 Token -> 请求 2 创建 Agent -> 请求 3 测试 Agent）。我希望工具在解析同一个文件时，能通过 `@depends-on` 确定同文件内的执行顺序，并将变量流畅传递。

### 📖 US 3: 大模型（LLM）多场景快捷初始化 (WBS 9.0)
*   **场景**：作为一个 AI 应用开发者，我希望通过一行命令（如 `rupost init llm`）在本地快速生成本地免密大模型测试（如 Ollama）、云端带秘钥测试（如 DeepSeek）以及中转代理接口测试（如 OneAPI）的三大典型开发模版，并且自动配备 `.env.example`，避免手动编写大量的 HTTP 头部和大模型 JSON Payload。

### 📖 US 4: SSE流输出物理同步落盘与实时渲染 (WBS 12.0)
*   **场景**：在调试大模型 Prompt 时，我希望使用 `@stream_to` 实时将流式响应的 Token 追加或覆写写入本地指定的 Markdown 文件，配合 IDE 的分屏渲染，实时查看 Prompt 调优的文档渲染效果。

### 📖 US 5: 流式数据中的变量捕获与断言 (WBS 7.0, 12.0)
*   **场景**：在测试 Agent 场景时，大模型的响应为流式（SSE）。我希望能够在流结束时，从拼接出的完整 LLM text 中通过 JSONPath 或 Regex 捕获内容，或者是只提取特定 event 类型的 JSON 字段，并注入到下一个普通请求中使用。

### 📖 US 6: 长连接中转与多协议高亮支持 (WBS 4.0, 13.0)
*   **场景**：作为一名全栈开发，在调试长连接或 GraphQL 时，我希望工具能支持 WS/WSS 监听与数据修改中转、兼容 MQTT 消息推送，并对 GraphQL Query / Mutation 语法提供原生的格式化与智能补全。

### 📖 US 7: 生产日志一键转测试用例与本地排障重现 (WBS 3.0)
*   **场景**：作为一名排障开发，我希望当线上环境报错时，能拉取生产日志并自动将其逆向转化为可在本地运行的 RuPost 请求用例。甚至重现一段时间内 WebSocket 的所有交互数据流，在开发环境进行故障重演。

### 📖 US 8: 跨工具历史迁移与 Markdown 成果规范导出 (WBS 2.0)
*   **场景**：作为从 Postman/Bruno 迁移来的用户，我希望可以导入旧工具的导出文件；在用 RuPost 调试好测试后，能将请求和断言一键导出为标准的 Markdown 代码块以提交至 Git 库。

### 📖 US 9: 插件热插拔机制与应用级别安全签名 (WBS 1.0)
*   **场景**：作为平台集成者，我希望可以通过开放的 JS/Rust API 开发服务端插件，并通过命令行进行安装/卸载。同时插件必须经过签名校验，防止非官方插件注入安全后门。

### 📖 US 10: 数据的物理脱敏与企业级合规审计 (WBS 14.0, 15.0)
*   **场景**：作为一名企业安全合规官，我希望 RuPost 的持久化历史和命令行报告中，手机号、API-Key 等敏感信息能够被自动按照规则脱敏。且所有测试运行均生成带有时间戳、身份识别的行为审计日志以备审查。

### 📖 US 11: API 设计先行与契约 Mock 变体服务 (WBS 9.0)
*   **场景**：我希望用同一个 Markdown 文件（如 `api-docs.md`）定义 API 规范，它既能直接被测试执行，也能作为契约直接启动 Mock 服务。在 Mock 过程中，支持根据不同的 Body 属性匹配不同的响应变体分支，甚至能配置模拟网络延迟（`delay`），模拟弱网以验证客户端的稳定性。

### 📖 US 12: 深度网络耗时诊断与状态隔离 (WBS 10.0, 17.0, 19.0)
*   **场景**：在 API 耗时超长或失败时，我希望能在终端或测试生成的报告中看到 DNS 解析耗时、TCP 握手时长、SSL 握手时长和 TTFB (首字节耗时) 的指标瀑布图，且在即时命令行中能够通过参数（如 `--no-cookies`）强制隔离本地 Cookie 以排出状态干扰。

---

## 3. 系统数据结构与 API 深度走查 (Walkthrough)

依据上述 12 个全覆盖 User Story，对现有系统的底层设计（如 `ParsedRequest`、`TestExecutor`、`VariableContext` 等）进行走查，发现以下关键技术 Gap：

### 🔍 走查 Gap 分析记录

| 故事 ID | 数据结构走查 (现有支持 vs 规划要求) | API 接口走查 (现有接口 vs 规划要求) | 关键差距 (Gap) |
| :--- | :--- | :--- | :--- |
| **US 1, US 2** | `ParsedFile` 仅包含扁平的 `requests: Vec<ParsedRequest>`，缺失“请求块”之间的拓扑依赖声明（`depends_on` 属性）；此外多文件依赖排序局限在文件级。 | `TestExecutor::execute_all` 只支持按顺序执行单文件请求。缺少跨文件参数共享、跨文件逻辑流转的核心 API。 | **控制流与编排层缺失**：无法在单个文件内或跨文件执行精细的 DAG（有向无环图）拓扑级条件跳转。 |
| **US 3, US 4, US 5** | `VariableCapture` 不支持从流式 SSE 的分片包中聚合提取；缺少针对大模型 SSE 结束时以及特定事件触发时的生命周期定义。 | 流式输出处理只有全局的 `stream_to` 写盘，缺乏对流式数据的增量断言及 Token 提取。 | **流式变量捕获与生命周期缺失**：引擎没有向外层脚本/变量捕获组件暴露出 SSE 结束帧的挂载钩子。 |
| **US 6** | `Request` 只有 HTTP 的 method, url, headers, body。缺少 WebSocket 消息包结构（`WsMessage`）和 MQTT 相关的 `Topic/Payload` 数据模型。 | `Client::execute` 采用的是单次 request-response 异步阻塞模型。缺少 WebSocket 握手后的长连接会话管道句柄（Channel Handler）以及中转拦截队列。 | **通信协议模型过窄**：底层引擎只支持 HTTP 协议家族，没有为长连接和流协议留出底层 socket 控制权。 |
| **US 7** | `RequestSnapshot` 只记录了最基础的 HTTP 请求。对于 WebSocket 数据流（带时间戳的消息回放序列），没有支持录制和持久化的数据结构。 | 缺少日志解析器（Log Parser）API，无法实现生产 HTTP 日志到 `ParsedRequest` 的逆向工程还原。 | **流式数据录制结构缺失**：不支持时间戳关联的消息回放和日志逆向解析。 |
| **US 9** | 现有 `TestExecutor` 中的 `Middleware` 是静态绑定的 Rust Trait。没有运行时动态分发的插件驱动（Plugin Driver）机制。 | 缺少 Plugin 管理器，没有任何与动态执行语言沙箱（如 WASM 或 JS 引擎）对接的 API。 | **动态插件加载器缺失**：不支持运行时热拔插的动态插件。 |
| **US 10** | `Request` 和 `Response` 没有加密、混淆或掩码字段。`history.jsonl` 保存的完全是明文敏感内容。 | 没有数据脱敏过滤器（DataMasker）挂载机制。没有向外输出企业级合规日志的审计拦截器（Auditor）。 | **合规隔离机制缺失**：持久化数据和控制台输出没有经过混淆脱敏，无行为审计切面。 |
| **US 11** | `ParsedMockVariant` 仅有简易的 `condition_expr` (单一的字符串表达)，难以支撑复杂 Mock；且无网络时延属性。 | 缺少 API 逆向生成、Mock 服务的路由分发引擎（Mock Router）。 | **Mock 变体匹配度过低**：缺乏复杂的 Body 匹配引擎与延时发生器。 |
| **US 12** | `TestResult` 只有 `duration` (总耗时)，无法体现网络细节。而 `rupost diagnose` 子命令拥有对 DNS、TCP、TLS、TTFB 耗时的收集逻辑，但这部分信息未向日常测试报告层输出。 | `TestExecutor::execute_one` 在执行测试时没有收集详细的网络耗时并填充到 `TestResult`。 | **诊断数据共享层缺失**：日常测试用例执行时发生了网络层面变慢时，没有复用诊断模块的指标数据。 |

---

## 4. 针对走查问题的系统架构修改建议

### 🛠️ 建议 1：复用与共用网络耗时诊断数据结构（解决 US 12）
> [!IMPORTANT]
> **关于网络指标复用设计**：
> `rupost` 的 `diagnose` 子命令目前已经在 [src/http/diagnose.rs](file:///Users/zsyzzx/project/rust/rupost/src/http/diagnose.rs) 中定义了 `DiagnosticsReport` 结构体：
> 
> ```rust
> pub struct DiagnosticsReport {
>     pub url: String,
>     pub is_https: bool,
>     pub resolved_ips: Vec<String>,
>     pub dns_lookup_duration: Duration,
>     pub tcp_connect_duration: Duration,
>     pub tls_handshake_duration: Option<Duration>,
>     pub cert_info: Option<CertInfo>,
>     pub http_status: Option<u16>,
>     pub http_version: Option<String>,
>     pub ttfb: Option<Duration>,
>     pub total_duration: Duration,
> }
> ```
> 
> 为了不引入冗余代码并保证 DRY (Don't Repeat Yourself) 原则，我们决定**不创建独立的 `NetworkDiagnostics` 结构体**，而是通过重构直接复用该结构体：
> 1. 将 `DiagnosticsReport` 拆分出更为精简的时间计量结构 `NetworkTiming`（或直接将 `DiagnosticsReport` 作为 `TestResult` 的可选子结构）。
> 2. 当 `rupost test` 开启了 `--debug` / `--debug-on-failure`，或者请求发生网络层失败时，`TestExecutor` 底层在发起网络请求时同样去填充该指标，并在 `TestResult` 中输出。
> 
> **架构修改方案**：
> ```rust
> // src/http/diagnose.rs (或移入更基础的通用层)
> #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
> pub struct NetworkTiming {
>     pub dns_lookup_duration: Duration,
>     pub tcp_connect_duration: Duration,
>     pub tls_handshake_duration: Option<Duration>,
>     pub ttfb: Option<Duration>,
>     pub total_duration: Duration,
> }
> 
> // 重构后的 DiagnosticsReport
> #[derive(Debug, Clone, Serialize, Deserialize)]
> pub struct DiagnosticsReport {
>     pub url: String,
>     pub is_https: bool,
>     pub resolved_ips: Vec<String>,
>     pub timing: NetworkTiming, // 复用时间定义
>     pub cert_info: Option<CertInfo>,
>     pub http_status: Option<u16>,
>     pub http_version: Option<String>,
> }
> 
> // src/runner/types.rs
> #[derive(Debug, Clone)]
> pub struct TestResult {
>     pub request_number: usize,
>     pub name: Option<String>,
>     pub method: String,
>     pub url: String,
>     pub status: Option<u16>,
>     pub duration: Duration,
>     pub success: bool,
>     pub error: Option<String>,
>     // ...
>     /// 新增：与诊断命令完全共用的网络耗时细节 (仅在需要时填充)
>     pub timing_details: Option<NetworkTiming>,
> }
> ```

---

### 🛠️ 建议 2：将 `VariableCapture` 升级以支持大模型流式生命周期（解决 US 5）
为 `CaptureSource` 引入流式特定的挂载点，区分流式响应（结束时拼接）与普通响应体：
```rust
// src/variable/capture.rs
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CaptureSource {
    /// 普通 JSON 响应体
    Body(String),
    /// 响应头
    Header(String),
    /// 正则表达式
    Regex(String),
    /// Cookie
    Cookie(String),
    /// 大模型 SSE 流数据捕获
    Stream {
        /// 捕获目标类型：文本拼接或原始事件
        target: StreamCaptureTarget,
        /// 可选过滤特定的 Event 类型 (例如 "message")
        event_filter: Option<String>,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StreamCaptureTarget {
    /// 自动捕获拼接出完整的 LLM Content
    LlmContent,
    /// 捕获全部原始的 SSE data 流
    RawEvents,
}
```

---

### 🛠️ 建议 3：在 HTTP 文件中引入“请求级”拓扑依赖（解决 US 1 & US 2）
在 `RequestMetadata` 级别引入 `depends_on`，用于指示同一个 HTTP 文件内多个请求块的串联顺序，并设计 Cookie 状态继承标志：
```rust
// src/parser/types.rs
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RequestMetadata {
    pub name: Option<String>,
    // ...
    
    /// 新增：当前请求依赖的同一个文件内前置请求块的 @name
    pub depends_on: Option<String>,
    
    /// 并行执行时，是否将依赖请求的 CookieJar 复制继承过来
    pub inherit_cookie_jar: bool,
}
```
通过该结构改造，测试流水线不再只能按物理文本位置从上到下死板执行，而是可以通过有向图（DAG）实现条件跳过、状态级联传递和局部并行化。

---

### 🛠️ 建议 4：完善 `ParsedMockVariant` 并挂载模拟网络时延（解决 US 11）
扩展 Mock 响应的分支条件匹配和网络时延参数：
```rust
// src/parser/types.rs
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ParsedMockVariant {
    /// 匹配键值对 (支持 JSON 路径匹配，如 body.model == "qwen")
    pub condition_rules: HashMap<String, serde_json::Value>,
    /// 模拟时延，用以测试大模型长连接或普通 API 在弱网环境下的重连与超时行为 (毫秒)
    pub delay_ms: Option<u64>,
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}
```

---

### 🛠️ 建议 5：底层多协议统一与 `ConnectionHandle` 抽象（解决 US 6）
在 `src/http/` 之上抽象出多协议通信层。
```rust
pub enum Connection {
    Http(HttpClient),
    WebSocket(WsClient),
    Mqtt(MqttClient),
}

pub struct WsMessageSnapshot {
    pub direction: MessageDirection,
    pub timestamp: u64,
    pub payload: Vec<u8>,
}
```
当发起 WebSocket 测试时，API 依然返回 `TestResult`，但它能返回一个持有长连接管道的 `ConnectionHandle`，支持后续的交互控制。
