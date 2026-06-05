# RuPost 全量后续规划用户故事（User Story）与系统架构走查报告

本报告根据 [project_next_state.md](file:///Users/zsyzzx/project/rust/rupost/doc/project_next_state.md) 中的 20 项后续功能规划，扩展和建立更完善的用户画像（User Personas），推导出 10 个覆盖完整使用生命周期的 User Story，并对照当前系统底层的数据结构与 API 进行全方位的深度走查与 Gap 分析，最后针对核心问题提出具体的系统架构演进修改建议。

---

## 1. 深度扩展的用户画像 (User Personas)

### 👤 后端研发工程师 - 小李 (Li) — 核心接口与性能保障
*   **痛点**：日常接口联调烦琐；复杂的微服务时序难以追溯；面临高并发或限流的生产问题时，缺乏排障日志和重现请求的手段。
*   **关联规划**：生产调试模式 (3)、WebSocket/MQTT (4)、高级断言 (7)、接入 AI (12)、自动化与性能测试 (16, 17)。

### 👤 前端研发工程师 - 小张 (Zhang) — 联调协作与快速 mock
*   **痛点**：接口文档经常发生静默变更导致前端报错；缺少轻量级的 Mock API 独立验证逻辑；长连接（WebSocket）及 GraphQL 的调试门槛高。
*   **关联规划**：导入/导出 (2)、WebSocket/GraphQL (4, 13)、API设计与 Mock (9)、TUI界面与主题 (10, 20)。

### 👤 测试自动化与安全工程师 - 小赵 (Zhao) — 质量与 CI/CD 守护者
*   **痛点**：庞大的测试套件之间缺乏依赖拓扑（Dependency Graph）编排，导致登录失效后全量用例挂掉；缺乏安全扫查（注入等）和数据脱敏。
*   **关联规划**：文件夹并行/串行测试 (5)、高级断言与流程控制 (7)、CI/CD 自动化 (8)、安全测试与自动化测试 (16, 18)。

### 👤 企业安全合规与平台管理员 - 老孙 (Sun) — 插件生态与企业合规 (新增画像)
*   **痛点**：接口调试工具缺乏审计日志，核心数据易泄漏；非授权的第三方插件可能引入代码注入风险；项目不同分支的测试状态容易冲突。
*   **关联规划**：插件系统与插件安全 (1)、版本管理与多工作区 (11)、企业审计与合规 (14)、数据脱敏 (15)。

---

## 2. 覆盖 20 项功能规划的 10 个 User Story

### 📖 US 1: 文件夹测试的依赖拓扑与执行流编排 (WBS 5.0, 7.0, 16.0)
> **场景**：作为测试开发，我希望可以一键按**顺序或并行**运行测试文件夹下的多个测试用例，且支持文件之间的依赖（如 `a.http` 执行成功后才执行 `b.md`）、条件控制（If/Else）和循环执行，以覆盖完整的复杂业务流断言。

### 📖 US 2: 长连接中转与多协议高亮支持 (WBS 4.0, 13.0)
> **场景**：作为一名全栈开发，在调试长连接或 GraphQL 时，我希望工具能支持 WS/WSS 监听与数据修改中转、兼容 MQTT 消息推送，并对 GraphQL Query / Mutation 语法提供原生的格式化与智能补全。

### 📖 US 3: 生产日志一键转测试用例与本地排障重现 (WBS 3.0)
> **场景**：作为一名排障开发，我希望当线上环境报错时，能拉取生产日志并自动将其逆向转化为可在本地运行的 RuPost 请求用例。甚至重现一段时间内 WebSocket 的所有交互数据流，在开发环境进行故障重演。

### 📖 US 4: 跨工具历史迁移与 Markdown 成果规范导出 (WBS 2.0)
> **场景**：作为从 Postman/Bruno 迁移来的用户，我希望可以导入旧工具的导出文件；在用 RuPost 调试好测试后，能将请求和断言一键导出为标准的 Markdown 代码块以提交至 Git 库。

### 📖 US 5: 插件热插拔机制与应用级别安全签名 (WBS 1.0)
> **场景**：作为平台集成者，我希望可以通过开放的 JS/Rust API 开发服务端插件，并通过命令行进行安装/卸载。同时插件必须经过签名校验，防止非官方插件注入安全后门。

### 📖 US 6: 数据的物理脱敏与企业级合规审计 (WBS 14.0, 15.0)
> **场景**：作为一名企业安全合规官，我希望 RuPost 的持久化历史和命令行报告中，手机号、API-Key 等敏感信息能够被自动按照规则脱敏（如 `138****0000`）。且所有测试运行均生成带有时间戳、身份识别的行为审计日志以备审查。

### 📖 US 7: 多工作区分支隔离与 JJ 多环境共存 (WBS 11.0)
> **场景**：作为一名高级开发，我希望可以在同一个物理工程下，像使用 `git worktree` 一样建立多个共存的工作区，各自拥有独立的测试快照历史、环境变量与 Cookie 存储，且不互相干扰。

### 📖 US 8: AI 驱动的智能生成、诊断与影子提示 (WBS 12.0)
> **场景**：作为一名追求极简的工程师，我希望能够通过自然语言指引 AI 帮我自动补全断言规则、智能解析报错的网络层并给出诊断意见，以及自动生成 Mock API 服务。

### 📖 US 9: API 设计先行（Mock & Code Gen） (WBS 9.0)
> **场景**：作为一名系统架构师，我希望在编写代码前在 RuPost 里设计 API Schema。工具能根据文档自动起一个 Mock 服务提供给前端，并在联调通过后自动生成 Rust/Go 后端骨架代码。

### 📖 US 10: 个性化 TUI Dashboard 与性能压测 (WBS 10.0, 17.0, 19.0, 20.0)
> **场景**：作为一名性能测试人员，我希望可以自定义 TUI 界面主题，在终端内直观看到高并发下的 QPS、吞吐率指标波动曲线，并支持将性能报告一键同步并下载到移动端。

---

## 3. 系统数据结构与 API 深度走查 (walkthrough)

依据上述 10 个全覆盖 User Story，对现有系统的底层设计（如 `Request`、`Response`、`ParsedRequest`、`TestExecutor`、`VariableContext` 等）进行走查，发现以下关键技术 Gap：

### 🔍 走查 Gap 分析记录

| 故事 ID | 数据结构走查 (现有支持 vs 规划要求) | API 接口走查 (现有接口 vs 规划要求) | 关键差距 (Gap) |
| :--- | :--- | :--- | :--- |
| **US 1** | 现有 `ParsedFile` 仅包含扁平的 `requests: Vec<ParsedRequest>`。缺少表示文件依赖关系的拓扑图模型，也缺少条件跳转（If/Else）指令的数据表示。 | 现有 `TestExecutor::execute_all` 只支持按顺序执行单文件请求。缺少跨文件参数共享、跨文件逻辑流转的核心 API。 | **控制流与编排层缺失**：无法进行跨文件依赖流的 DAG（有向无环图）拓扑解析和跳过/跳转。 |
| **US 2** | `Request` 只有 HTTP 的 method, url, headers, body。缺少 WebSocket 消息包结构（`WsMessage`）和 MQTT 相关的 `Topic/Payload` 数据模型。 | `Client::execute` 采用的是单次 request-response 异步阻塞模型。缺少 WebSocket 握手后的长连接会话管道句柄（Channel Handler）以及中转拦截队列。 | **通信协议模型过窄**：底层引擎只支持 HTTP 协议家族，没有为长连接和流协议留出底层 socket 控制权。 |
| **US 3** | `RequestSnapshot` 只记录了最基础的 HTTP 请求。对于 WebSocket 数据流（带时间戳的消息回放序列），没有支持录制和持久化的数据结构。 | 缺少日志解析器（Log Parser）API，无法实现生产 HTTP 日志到 `ParsedRequest` 的逆向工程还原。 | **流式数据录制结构缺失**：不支持时间戳关联的消息回放和日志逆向解析。 |
| **US 5** | 现有 `TestExecutor` 中的 `Middleware` 是静态绑定的 Rust Trait。没有运行时动态分发的插件驱动（Plugin Driver）机制。 | 缺少 Plugin 管理器，没有任何与动态执行语言沙箱（如 WASM 或 JS 引擎）对接的 API。 | **动态插件加载器缺失**：不支持运行时热拔插的动态插件。 |
| **US 6** | `Request` 和 `Response` 没有加密、混淆或掩码字段。`history.jsonl` 保存的完全是明文敏感内容。 | 没有数据脱敏过滤器（DataMasker）挂载机制。没有向外输出企业级合规日志的审计拦截器（Auditor）。 | **合规隔离机制缺失**：持久化数据和控制台输出没有经过混淆脱敏，无行为审计切面。 |
| **US 7** | 所有的配置目录都是写死的全局 `.rupost/`。缺少针对多分支、多工作区共存的分隔模型。 | 缺少工作区切换（`Workspace::switch`）和隔离文件指针的全局定位 API。 | **多工作区定位器缺失**：缺少多工作区管理的逻辑隔离。 |
| **US 9** | 系统内目前没有定义 API 设计 Schema (OpenAPI/Swagger 等规格数据模型)。 | 缺少 API 逆向生成、Mock 服务的路由分发引擎（Mock Router）。 | **API 设计层缺失**：目前完全没有 Mock 和代码生成模块。 |

---

## 4. 针对走查问题的系统架构修改建议

### 🛠️ 建议 1：扩展底层通信协议层，引入 `ConnectionHandle` 抽象（解决 US 2, US 3）
*   **设计修改**：
    在 `src/http/` 之上抽象出多协议通信层 `Communication`。
    定义长连接状态模型：
    ```rust
    pub enum Connection {
        Http(HttpClient),
        WebSocket(WsClient),
        Mqtt(MqttClient),
    }
    
    pub struct WsMessageSnapshot {
        pub direction: MessageDirection, // Inbound / Outbound
        pub timestamp: u64,
        pub payload: Vec<u8>,
    }
    ```
*   **接口改造**：
    `Client::execute` 需要进化，不仅能返回单次 `Response`，对于 WS 等流式长连接，可以返回一个持有长连接控制权的句柄 `ConnectionHandle`，支持向流中注入、监听或复制数据包。

### 🛠️ 建议 2：设计工作流控制结构 `Workflow` 与 DAG 解析器（解决 US 1）
*   **设计修改**：
    引入 `Workflow` 概念，将文件夹内的测试用例解析为一个依赖图：
    ```rust
    pub struct WorkflowNode {
        pub file_path: PathBuf,
        pub depends_on: Vec<PathBuf>,
        pub condition: Option<String>, // 例如: "status == 200"
    }

    pub struct TestWorkflow {
        pub nodes: HashMap<PathBuf, WorkflowNode>,
    }
    ```
*   **WBS 演进**：
    在执行文件夹测试时，通过拓扑排序（Topological Sort）决定测试文件加载的先后顺序；并在单个 Node 执行失败后，由控制引擎决定是否跳过依赖于它的所有后置节点（拓扑级跳过）。

### 🛠️ 建议 3：在历史与数据写入层挂载数据脱敏拦截器 `DataMasker`（解决 US 6）
*   **设计修改**：
    在 `TestExecutor` 中引入数据混淆逻辑：
    ```rust
    pub trait DataMasker: Send + Sync {
        fn mask_request(&self, req: &mut Request);
        fn mask_response(&self, resp: &mut Response);
    }
    ```
*   在历史记录记录（`record_history`）和终端打印（`print_result`）的前一刻，将 Header 里的 Key、Token，以及 Body 中的特定正则表达式字段进行脱敏渲染，保障安全性。

### 🛠️ 建议 4：为插件系统预留 WASM/JS 运行时（解决 US 5）
*   **设计修改**：
    在中间件层，允许注册一个 `ScriptingMiddleware`。
    通过内置基于 `wasmer` 或 `rquickjs` 的解析运行时，使得第三方用 JS 编写的插件在安全的沙箱环境下能够获得 `Request` 和 `Response` 的内存副本进行修改。
