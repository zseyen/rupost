# RuPost 全生命周期用户故事与 compliance 走查报告

本报告根据 API 研发与运维的多维角色画像，提炼了覆盖设计、开发、联调、测试、CI/CD 自动化卡点、网络与代理调试、逻辑联动与数据初始化调试、**流式 (SSE) 与双向 WebSocket 契约审计**、以及服务演进下线等全生命周期的完整用户故事（User Story）。在此基础上，对 `rupost` 当前规划的数据结构、执行器模块及命令行 API 进行严格的架构走查，记录走查发现的隐藏漏洞并给出架构级修补建议。

---

## 👥 1. 用户画像定义 (User Personas)

为了确保用户故事能够覆盖所有工业级真实使用场景，我们定义以下三类典型用户画像：

| 用户画像 | 核心职责 | 技术关注点与痛点 |
| :--- | :--- | :--- |
| **Alex (小智) - 全栈开发** | 负责 API 设计、后端业务编写及与前端/客户端的高效联调。 | API 契约设计完后前端需要立刻调用模拟接口；后端编写测试用例时希望免去对每个字段重复写 `@assert`。 |
| **Beatrice (小美) - DevOps 专家** | 负责测试套件在 CI/CD 流水线中的集成与守卫；负责生产与测试现场的环境调试。 | 物理 `.md` 规范书因涉密不分发至流水线（仅有 `.http`）；需排除第三方 API 导致的干扰；已废弃接口能在流水线中被及时捕捉并拦截。 |
| **Charlie (老查) - 安全与三方集成架构师** | 负责与外部微服务及微信支付、GitHub 等三方系统集成；处理复杂的企业内网隔离与网络代理。 | 用例需灵活绕过企业自签名 CA 限制；特定请求需走代理网络进行穿透调试；时延需精准诊断；调试时需要执行复杂的数据初始化与接口调用联动；实现复杂的异步事件流 (SSE) 和 WebSocket 长连接通信审计。 |

---

## 🗺️ 2. 全生命周期场景下的 User Story 定义

### 📖 Story 1: 【API 设计阶段】规格定义与一致性契约起草
*   **画像角色**：Alex (全栈开发)
*   **动作流程**：
    *   *门槛痛点*：手动编写带 YAML Frontmatter 且格式严格的 `.md` 文件比较繁琐，容易出错。
    *   *操作*：Alex 既可以在编辑器中新建 `order_spec.md` 编写 API 设计原型，也可以通过未来提供的**图形化设计器**（GUI 表单输入或 TUI 交互式向导），填写 API 路由和 Schema，由工具自动输出标准的契约 Markdown。
*   **验收标准**：设计元数据能作为 AI 智能体进行自动化编码与评审时的长效记忆；HTTP 块可做语法正确性检查。GUI/TUI 工具能自动读取或格式化输出合规的 `.md` 规格说明书。

### 📖 Story 2: 【前后端联调阶段】智能数据反射与 Mock 分流
*   **画像角色**：Alex (全栈开发)
*   **动作流程**：
    *   Alex 直接在 `order_spec.md` 的 HTTP 块内编写 Mock 变体配置：
        *   当传入参数 `item_count > 100` 时返回大客折扣响应；
        *   支持动态占位符（如随机订单 ID `{{$random_id}}`）。
    *   *空 Body 智能合成*：若接口未定义 mock body，但在 spec 中有 Schema，支持基于 Schema 自动合成零 Body 的语义测试数据。为了能够应对未来更复杂的生成算法，Mock 引擎预留了可插拔的 `MockGeneratorPlugin` 生成接口。
    *   运行 `rupost mock start order_spec.md --port 9000`。
*   **验收标准**：前端直接请求 `http://localhost:9000/api/v1/orders`，Mock 服务器能根据入参条件自动路由命中的响应变体。若使用空 Body 合成，将路由至装载的生成插件产生合规假数据。

### 📖 Story 3: 【用例开发与物理分发阶段】测试继承与 CI/CD 物理分发打包
*   **画像角色**：Alex (全栈开发) & Beatrice (DevOps 专家)
*   **动作流程**：
    *   *多环境 Git 管理*：在 Git 代码库中集中式存放资产（设计书统一放在 `doc/specs/`，用例统一放在 `tests/integration/`）。
    *   *继承关联*：在用例头部声明 `### @extends-spec ../../doc/specs/order_spec.md (version >= 1.2.0)` 建立继承关系。
    *   *编译导出与裁剪分发*：为保障核心设计文档不泄露到部署现场，Beatrice 在 CI/CD 打包流水线中运行 `rupost export doc/specs/ -o tests/integration/ --all-in-place`，将契约就地编译嵌入到所有 `.http` 文件顶部（行特征匹配 `# @spec-` 无损回写）。**打包发布时，打包脚本显式过滤/排除整个 `doc/specs/` 目录**，仅将 `tests/integration/` 目录打包拷贝至分发包中分发。
*   **验收标准**：
    *   物理 `.md` 规范说明书完全从生产和测试现场物理隔离，保障敏感设计信息安全。
    *   导出的 `.http` 顶部以普通的 `# @spec-` 行内注释承载契约快照，不包含任何突兀的 RuPost 专有边界行，保持 100% 的通用 HTTP 客户端运行兼容性。

### 📖 Story 4: 【流水线验证阶段】无盘契约回归与 Semver 版本校验
*   **画像角色**：Beatrice (DevOps 专家)
*   **动作流程**：
    *   现场环境中没有 `order_spec.md`。流水线运行 `rupost test order_test.http`。
*   **验收标准**：
    *   系统自适应识别到本地物理 `.md` 缺失，降级解析 `order_test.http` 顶部的内联契约注释构建内存契约库。
    *   版本及 Schema 超集比对成功通过。
    *   第三方外部请求（Host 不匹配且契约未声明）自动激活智能逃逸，直接通过不引发校验报错。

### 📖 Story 5: 【网络与逻辑调试阶段】网络代理与逻辑联动调试
*   **画像角色**：Charlie (安全与集成架构师)
*   **动作流程**：
    *   *安全与代理调试*：Charlie 对使用自签名证书的本地测试接口在用例首部声明 `@insecure` 绕过校验；对于需要走代理穿透的三方接口，局部使用 `@proxy` 强制走网络代理。
    *   *数据初始化与清理 (Setup & Teardown)*：调试前，通过 `### @depends-on db_init.http` 拓扑运行数据准备脚本，或使用前置/后置逻辑触发状态清空。
    *   *接口联动与变量捕获 (Chaining)*：Charlie 调试“订单->退款”流，使用 `@capture refund_token from body.token` 捕获响应字段，并在下一个退款用例中通过 `{{refund_token}}` 联动引用。
*   **验收标准**：
    *   引擎在执行测试时，精准响应请求级别的网络配置。
    *   引擎提供跨请求的文件域变量上下文持久化与流转，确保联动链条和初始化脚本有序执行。

### 📖 Story 7: 【流式推送与双向通信校验】SSE 帧审计与 WebSocket 双向帧拦截
*   **画像角色**：Charlie (安全与集成架构师) & Alex (全栈开发)
*   **动作流程**：
    *   *SSE 推送审计*：用例发起对 `/api/v1/orders/stream` 的流式接收。随着时间推移，服务端会异步向客户端推送诸如 `order_created` 与 `order_paid` 等不同类型的事件帧。
    *   *WebSocket 双向拦截*：在 `/api/v1/orders/ws` 双向通道中，客户端执行 `@send` 发送订阅消息，服务端则持续推送交易更新和错误提示帧。
*   **验收标准**：
    *   **现场脱盘闭环运行**：在缺失原 `.md` 物理契约的集成测试现场，校验器完美通过内联快照中的 `# @spec-route = ... (sse: true, events: ...)` 还原复杂的流控制策略。
    *   **SSE 逐帧审计**：接收到 SSE 流帧后，根据每一帧的 `event` 名称自适应分流，强行匹配对应事件的 JSON 弱 Schema，任何一帧出错都会高亮输出。
    *   **WebSocket 双向审查**：用例向外 `@send` 数据前，拦截并比对 `client_send` 中对应 `type` 的 Schema（防止非法客户端消息逃逸）；接收到推送数据后，拦截并比对 `server_push` 对应 `type` 的 Schema。

### 📖 Story 6: 【服务演进与下线阶段】废弃 API 彩色预警与 CI 熔断
*   **画像角色**：Alex (全栈开发) & Beatrice (DevOps 专家)
*   **动作流程**：
    *   旧接口被弃用，打上 `@deprecated Use POST /orders/v2 instead` 标记。
    *   Beatrice 运行测试套件。如果在 `rupost.toml` 中配置了 `fail_on_deprecation = true`。
*   **验收标准**：
    *   控制台以黄色高亮打印废弃详情警告。
    *   若熔断配置为 `true`，触发废弃 API 测试时，套件执行完后返回非零退出码，阻断流水线继续发布。

---

## 🔍 3. 架构与 API 走查记录 (Walkthrough)

对照上述 7 个覆盖全生命周期的 User Story，我们对现有的数据结构（`src/parser/types.rs`、`src/parser/metadata.rs`）、运行时模块及命令行 API 设计进行走查：

### 🔎 3.1 走查 1：`ParsedFile` 与 `FileMetadata` (对齐 Story 1 & Story 4)
*   **走查问题评估**：
    *   *问题 A (Host 缺失)*：契约元数据中缺少 `host`。没有 `host` 的支持，Story 4 中第三方 URL 逃逸校验时，引擎无法判断哪个 Host 是“本地主服务域名”。
    *   *问题 B (单一继承防冲突)*：未体现如何防止一个文件重复引用多个 spec 导致的冲突。
*   **修改建议**：
    *   在 `FileMetadata` 增加 `pub host: Option<String>`。
    *   在 `ParsedFile` 增加 `pub extends_spec_path: Option<PathBuf>` 和 `pub extends_spec_version_req: Option<semver::VersionReq>`。
    *   在解析器中，如果一个用例文件扫描出了多行 `### @extends-spec`，立即抛出 `ParseError::InvalidMetadata` 错误阻断，只允许单继承。

### 🔎 3.2 走查 2：`RequestMetadata` 与网络/调试指令 (对齐 Story 5 & Story 6)
*   **走查问题评估**：
    *   *问题 C (网络与逻辑指令缺失)*：缺少对 `@insecure`、`@proxy`、`@no_proxy` 的支持。没有这些字段，Story 5 的网络代理分流和安全证书逃逸将无法在 `Request` 级别应用。
    *   *问题 D (废弃标记缺失)*：缺少 `@deprecated` 字段，阻碍了 Story 6 对废弃接口高亮报警的支持。
*   **修改建议**：
    *   在 `RequestMetadata` 结构体中新增 `insecure: bool`, `proxy: Option<String>`, `no_proxy: bool`, `deprecated: Option<String>`。
    *   在 `src/parser/metadata.rs` 的 `parse_metadata` 和 `apply_metadata` 函数中追加这些元数据指令的拦截、解析与填充逻辑。

### 🔎 3.3 走查 3：Kahn 依赖图构建与 DAG 调度 (对齐 Story 4)
*   **走查问题评估**：
    *   *问题 E (DAG 死锁与环境崩溃)*：由于 `@extends-spec` 指向的通常是 `.md` 说明书，如果在测试现场没有 `.md` 文件，DAG 构建就会因为找不到该依赖节点而发生致命错误。
*   **修改建议**：
    *   在 `HttpFileParser::extract_dependencies` 中，**只能**将带有 `@depends-on` 特征的路径加入 `dependencies` 中。
    *   `@extends-spec` 的解析结果**仅仅**写入到专用的 `ParsedFile::extends_spec_path` 字段中，从拓扑排序图中物理隔离。

### 🔎 3.4 走查 4：`rupost mock` 从内嵌快照中拉起 Mock 服务器 (对齐 Story 2 & Story 7)
*   **走查问题评估**：
    *   *问题 F (现场 Mock 功能瘫痪与可插拔生成预留)*：在测试现场物理 `.md` 缺失时，用户无法使用内嵌有 `# @spec-route = ...` 的用例拉起 Mock 服务器。此外，如何优雅预留数据自动合成的第三方插件扩展点？
*   **修改建议**：
    *   在 `ParsedFile` 内部定义 `pub embedded_spec: Option<EmbeddedSpec>`。
    *   扩展 `src/mock/compiler.rs` 中的 `MockCompiler`。当发现输入文件没有物理 `.md`，但 `embedded_spec` 存在时，遍历 `embedded_spec.routes` 并将其转换为轻量的 `MockRouteConfig`：
    *   **预留接口**：在 `rupost` 库核心层定义 `MockGeneratorPlugin` trait，提供 `generate_mock_body` 接口。引擎默认装载基于弱 Schema 自研递归推断的生成插件，且允许在 `rupost.toml` 中配置注册外部第三方生成插件。

### 🔎 3.5 走查 5：流式与长连接协议的内联快照表示 (对齐 Story 7)
*   **走查问题评估**：
    *   *问题 G (流协议快照解析与比对漏洞)*：目前的 `ParsedRequest` 对 WebSocket/SSE 的元数据虽然解析了标记，但在契约匹配时，无法自动将普通的 JSON 响应 Schema 匹配扩展为支持 SSE 异步分流和 WebSocket 双向推送消息的精细校验结构。
*   **修改建议**：
    *   定义 `EmbeddedSpecRoute` 支持解析和携带 SSE 与 WebSocket 特有契约字段：
        ```rust
        pub struct EmbeddedSpecRoute {
            pub method: String,
            pub path: String,
            pub schema: Option<serde_json::Value>,
            pub deprecated: Option<String>,
            // 新增流式与长连接支持
            pub sse: bool,
            pub sse_events: std::collections::HashMap<String, serde_json::Value>,
            pub websocket: bool,
            pub ws_client_send: std::collections::HashMap<String, serde_json::Value>,
            pub ws_server_push: std::collections::HashMap<String, serde_json::Value>,
        }
        ```
    *   在 `src/parser/http_file.rs` 解析 `.http` 顶部的 `# @spec-route` 时，增强分词与提取引擎，解析内嵌的 `events: {...}`、`client_send: {...}` 与 `server_push: {...}` 的 Schema 对象。
