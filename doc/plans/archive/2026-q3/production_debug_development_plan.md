# RuPost 生产调试模式——基于 MVP 的原子化开发实施计划 (修订版)

本计划结合 **MVP (最小可行性产品)** 规则与**“可以直接发布”(Shippable Increments)** 的交付理念，将“生产调试与开发排障模式”拆解为 8 个独立的开发阶段。

---

## 🔍 阶段 1 深度解析：本地局部环境变量覆盖与热重载

### 1. 详细使用场景 (Use Cases)
*   **痛点**：在微服务敏捷团队中，共享配置 `rupost.toml` 中配置了 dev 共享网关 `http://dev-gateway.internal:8080` 和全局测试 API Key。后端开发小李在本地修改了 `order-service` 代码并启动在本地的 `http://localhost:8081`。如果小李为了本地自测而直接修改并提交 `rupost.toml` 中的 `base_url`，会导致 Git 冲突，影响团队联调。
*   **解决方案**：小李在本地创建一个被 Git 忽略的 `.env` 文件：
    ```env
    base_url=http://localhost:8081
    api_key=my-local-debug-key
    ```
    当他运行 `rupost test api.md --env dev` 时，RuPost 自动检测并合并该局部变量，级联覆盖掉全局配置中的 `base_url`。
*   **级联优先级顺序 (Cascading Priority)**：
    `命令行覆盖 (--var key=val)` > `系统环境变量` > `本地局部 .env 文件` > `共享 rupost.toml 环境配置`

### 2. 测试内容与设计 (Testing Specification)
*   **单元测试 (Unit Tests)**：
    - `test_env_parser_valid_and_invalid`：校验 `.env` 解析器，处理包含双引号、尾随空格、特殊字符或空行等临界情况。
    - `test_cascading_priority_logic`：在内存中构建 `VariableConfig`、`.env` 键值对映射、命令行 `cli_vars`，依次注入并验证输出的 `VariableContext` 中的最终值完全符合覆盖优先级。
*   **集成测试 (Integration Tests)**：
    - `test_local_run_overrides_base_url_success`：使用 `tempfile` 在测试沙箱中动态创建临时 `.env` 文件。拉起本地 `wiremock` 作为 Mock 服务，运行测试，断言请求最终成功发送到了由 `.env` 重写的端口，验证整个解析与网络发包流的闭环。

---

## 🛠️ Mock 相关功能的深度设计

在 **阶段 7** 中，我们对 Mock 模块进行以下深度扩展设计，使其能够承担复杂的生产级流量代理匹配：

### 1. 基于 Trie 树的模糊路径匹配器 (`MockRouteMatcher`)
对于路径中带有动态 ID 或 Query 的情况，引入通配符匹配和路径参数捕获：
*   **Trie 树数据结构**：
    ```rust
    pub enum RouteSegment {
        Literal(String),
        Param(String),      // 例如 ":id"
        Wildcard,           // 例如 "*" 或 "**"
    }
    
    pub struct MockRouteNode {
        pub segment: RouteSegment,
        pub handler: Option<MockRouteHandler>,
        pub children: Vec<MockRouteNode>,
    }
    ```
    支持将请求 `/api/users/123/billing?_t=98765` 模糊匹配到配置好的 `/api/users/:id/billing` 录制快照路由中。

### 2. 多路分支条件匹配变体 (`MockVariant`)
同一个 API 端点对于不同的输入应该有不同的响应（如用户名 admin 返回管理界面，guest 返回普通用户界面）。
*   **数据模型**：
    ```rust
    pub struct MockVariant {
        /// 判定条件，使用简单的字段匹配规则或 JSONPath 规则
        pub condition: Option<VariantCondition>,
        pub status: u16,
        pub headers: HashMap<String, String>,
        pub response_body: String,
    }
    
    pub struct VariantCondition {
        pub source: ConditionSource, // Body, Header, Query
        pub path: String,            // 例如 JSONPath: "$.user_type"
        pub expected_value: String,  // 例如 "admin"
    }
    ```
    Mock 服务器在匹配到路由后，将顺次评估 `MockVariant` 链，找到第一个满足 `condition` 的响应体进行返回。

---

## 🔄 各阶段之间的设计复用与可扩展性 (Clean Architecture)

为了规避低效的二次重构，我们在整个开发路线中贯彻高内聚、低耦合的复用性设计：

### 1. 变量上下文 (`VariableContext`) 的复用 (阶段 1 -> 阶段 5 & 7)
*   **复用机制**：阶段 1 产出的级联解析后的 `VariableContext` 具有统一的键值读取接口。
    - **在阶段 5**：重放拦截器管道直接利用此 Context 读取当前最新的 Token 并动态覆盖旧 Token。
    - **在阶段 7**：Mock 条件变体匹配引擎复用阶段 1 的 `VariableResolver` 去解析并比对 JSONPath 中的动态值。

### 2. 快照模型 (`Snapshot`) 的多态扩展 (阶段 3 -> 阶段 4, 7, 8)
*   **复用与扩展机制**：阶段 3 定义的快照读写库（`SnapshotRepo`）被作为 Infrastructure 层的核心数据源。
    - **在阶段 4**：`SnapshotRepo` 作为装饰器挂载加密解密逻辑，无需修改 Snapshot 实体。
    - **在阶段 7**：Mock Server 启动时直接调用 `SnapshotRepo::load` 读取录制数据，将其输入给 `MockRouteMatcher` 进行内存路由构建。
    - **在阶段 8**：通过将 `Snapshot` 中的 Payload 字段多态化（扩展为 WebSocket 时序帧），完美兼容长连接帧会话，核心读写接口无需重构。

### 3. 数据掩码拦截器 (`DataMasker`) 的复用 (阶段 4 -> 阶段 6)
*   **复用机制**：阶段 4 编写的 `DataMasker` 具有高度抽象的过滤方法。在阶段 6 执行 SSH/K8s 日志实时拉取并输出到 TUI 时，可以复用此 Masker 将控制台输出的日志中包含的密钥、密码进行防窥掩码处理，保证开发排障时的合规与安全。

---

## 阶段交付与验收规格书 (阶段 1-8)

### 📅 阶段 1：本地局部环境变量级联覆盖与热重载
*   **可以直接发布的状态**：
    - 支持 CLI 加载 `.env` 文件，并与系统环境变量和命令行 `--var` 级联合并。
    - 提供局部配置热重载 API。
*   **验收标准**：通过 `cargo test` 100% 测试通过。

### 📅 阶段 2：网络连通性分析与高亮诊断工具
*   **可以直接发布的状态**：
    - 新增独立子命令 `rupost diagnose <url>`。
    - 终端高亮输出 DNS 耗时、已解析 IP、TLS 版本及证书过期剩余天数。

### 📅 阶段 3：HTTP 请求/响应基础快照录制与原样重放
*   **可以直接发布的状态**：
    - 支持 `--save-snapshot [name]` 参数和独立命令 `rupost history replay <snapshot_file> --target <url>`。

### 📅 阶段 4：快照敏感数据脱敏混淆与对称加密
*   **可以直接发布的状态**：
    - 快照保存命令支持 `--mask` 和 `--encrypt`。
    - 敏感字段自动混淆脱敏，快照包经 AES-GCM 加密落盘。

### 📅 阶段 5：Replay 拦截器管道与动态凭证注入
*   **可以直接发布的状态**：
    - 引入拦截器机制，`rupost history replay` 命令自动从当前的局部变量中加载最新 Token 替换快照中的旧 Token。

### 📅 阶段 6：Trace ID 自动提取与远程多源日志聚合定位
*   **可以直接发布的状态**：
    - 自动提取 Traceparent 等字段，引入 `rupost logs show --trace-id <id> --source ssh` 远程多行过滤聚合。

### 📅 阶段 7：模糊路径匹配与条件变体独立 Mock 服务器
*   **可以直接发布的状态**：
    - 提供命令 `rupost mock start <snapshot_file> --port <port>`。
    - 支持 Trie 树模糊路由和多路分支条件匹配。

### 📅 阶段 8：WebSocket 双向长连接消息帧时序回放
*   **可以直接发布的状态**：
    - 支持 WebSocket 会话消息流录制（`rupost ws record`）与一比一递增延迟时序重放（`rupost ws replay`）。
