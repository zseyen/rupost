# RuPost 生产调试模式场景驱动 User Story 与架构走查报告

本报告针对 RuPost 项目规划中 3.0 版本的**生产环境调试与排障模式**进行全方位的深度分析。我们摒弃传统的孤立走查，采用**场景驱动开发 (Scenario-Driven Development)** 的第一性原理：由具体的开发者使用场景，推导出端到端的 User Story 及命令行/TUI 交互流，并以此走查规划中的核心数据结构与 API，找出其潜在的 Gap 与漏洞，最终提供具体的架构演进与修改建议。

---

## 1. 深度扩展的用户画像 (User Personas)

### 👤 后端排障工程师 - 小李 (Li) — 微服务健康与全链路追踪守护者
*   **痛点**：生产服务在特定高并发或复杂入参时报 500 错，但本地和 Staging 环境难以重现。面对分布式微服务集群，小李必须人肉 SSH 登录多台主机 grep 日志，或者在庞大的 ELK 平台里大海捞针般比对 Trace ID，极其耗时（通常 1-3 小时）。
*   **期望**：在请求失败的瞬间，工具能自动抓取并聚合关联的分布式多 Pod 日志和 Trace 链路，并在 3 分钟内定位根因。

### 👤 技术支持与 QA 工程师 - 小赵 (Zhao) — 线上问题捕获与回归测试员
*   **痛点**：客户在线上反馈某项业务失效。小赵需要将故障现场反馈给研发，但由于涉及复杂的认证 Token、Cookie、多环境敏感变量，人肉拼接 cURL 极其容易出错；同时直接将包含用户隐私（如支付 Token）的数据明文发送，会带来极大的数据合规（GDPR）风险。
*   **期望**：能一键将故障现场的请求、响应、环境变量脱敏打包成加密快照，开发在本地一键 Replay 即可完美复现。

### 👤 前端联调工程师 - 小张 (Zhang) — 前后端解耦与数据 Mock 专家
*   **痛点**：后端 API 还在高频开发或线上测试中，前端需要联调但缺乏真实的生产级复杂数据集（如包含几十项嵌套状态的订单账单数据）。如果在本地手写假数据，无法反映真实接口边界。
*   **期望**：能非侵入式地捕获生产环境响应，在本地一键起一个 Mock 服务器，根据路径自动返回录制的生产级响应，且不干扰后端运行。

### 👤 实时应用开发工程师 - 小王 (Wang) — 长连接时序排障专家
*   **痛点**：WebSocket 双向交互具有强烈的状态和时序依赖。当遇到连接异常断开（如心跳超时、特定交互帧格式错误）时，普通的单次 HTTP 请求-响应历史根本无法还原时序级的网络现场。
*   **期望**：能录制一段时间内 WebSocket 双向交互的完整时序帧，在本地高精度重放，以定位前后端状态机不一致的问题。

---

## 2. 场景驱动的 6 类核心 User Story 与 CLI 交互设计

### 📖 US 1：微服务 500 异常一键拉取关联日志与追踪 (微服务排障场景)
> **场景描述**：作为后端开发，当我在生产环境运行测试时，如果某个接口返回了 500 错误，我希望 RuPost 能够自动提取响应头中的 Trace ID，并通过预配置的 SSH、Kubernetes API 或外部日志平台，一键聚合拉取该请求在后端微服务集群中的关联日志和 Jaeger 追踪链路，免去登录服务器过滤日志的痛苦。

#### 💻 CLI 交互设计流：
```bash
# 1. 针对生产环境执行接口测试，开启自动健康检查与日志追溯
$ rupost test api/order.http --env prod --check-health --fetch-logs

[Dependency Health Check]
  ✓ auth-service (23ms)
  ✗ user-database (2345ms) - ⚠ DEGRADED (连接池满)
  ✓ cache-redis (5ms)

[Test Execution]
  ✗ Request #1: 创建订单接口 - FAILED
    Status: 500 Internal Server Error (Duration: 256ms)
    Request ID: req-order-123
    Trace ID: trace-payment-789 (提取自 Traceparent)

[Remote Logs - Kubetnetes Aggregated]
  Pod: order-service-7d9f8b6c4-abc12
  2026-06-05 23:10:15.167 WARN  [trace-payment-789] DB connection acquired timeout after 2000ms.
  2026-06-05 23:10:15.178 ERROR [trace-payment-789] Failed to create order: ConnectionPoolExhausted.

# 2. 查看详细分布式链路
$ rupost trace show --trace-id trace-payment-789

[Distributed Trace: trace-payment-789]
api-gateway (256ms) → order-service (230ms) → [FAILED] user-database
```

---

### 📖 US 2：故障现场脱敏打包与跨环境一键重现 (客服与技术支持场景)
> **场景描述**：作为技术支持，线上发生客户故障时，我希望通过一键保存包含完整上下文（Request、Response、Cookie、Env 局部快照）的 Snapshot，在自动掩码脱敏敏感信息后导出为加密文件。开发收到文件后在本地通过 `--target` 重定向到本地服务器一键重放，在开发沙箱中重现故障。

#### 💻 CLI 交互设计流：
```bash
# 1. 技术支持在生产环境运行测试，将故障现场打包为快照，并对 API-Key 和手机号自动掩码脱敏
$ rupost test api/login.http --env prod --save-snapshot error-现场 --mask "Authorization,phone" --encrypt

Snapshot saved and encrypted to: ~/.rupost/snapshots/error-現場.enc.json

# 2. 研发人员收到快照包后，解密并在本地开发环境一键回放
$ rupost history replay ~/.rupost/snapshots/error-現場.enc.json --env dev --target http://localhost:8080

[Replaying Snapshot: error-现场]
  Target: http://localhost:8080
  Context Restored: Headers (Masked applied), Cookie (Session-ID restored)
  
  Actual Response: 400 Bad Request
  Expected (Snapshot): 500 Internal Server Error
  
  [Diff Detected]
  - Response Body:
    - {"error": "Database error"}
    + {"error": "Invalid localized date format"} (本地暴露了时区解析 Bug)
```

---

### 📖 US 3：非侵入式生产流量捕获与本地 Mock 独立开发 (前端联调场景)
> **场景描述**：作为前端开发，在后端服务不稳定或数据构造困难时，我希望通过录制一段真实的生产环境 API 交互数据，在本地启动一个 Mock 服务器。该服务器能通过模糊匹配和通配符匹配规则，向前端返回录制的生产级响应，实现非侵入式的独立前端联调。

#### 💻 CLI 交互设计流：
```bash
# 1. 录制生产环境特定模块的交互响应数据
$ rupost record start --env prod --filter "/api/billing/**" --output billing-mock.json

[Recording Status]
  Captured 12 requests. Saved to billing-mock.json.

# 2. 在本地启动 Mock 服务，并指定匹配策略
$ rupost mock start billing-mock.json --port 9000 --match-strategy fuzzy

[Mock Server Running]
  Listening on http://localhost:9000
  Matching strategy: Fuzzy Path + HTTP Method
  
  Matched Route Demo:
  GET /api/billing/orders/123 -> Returning recorded response of order #123 (Wildcard user-id matched)
```

---

### 📖 US 4：发布前后 API 响应一致性与时延退化校验 (QA 回归场景)
> **场景描述**：作为 QA，在新版本发布到灰度或生产环境后，我希望将录制好的发布前基准（Baseline）快照在灰度环境进行重放，并比对响应的字段结构（JSON Schema）与时延。如果发现必填字段缺失或时延退化超过 50%，则触发报警拦截，防止带病发布。

#### 💻 CLI 交互设计流：
```bash
# 1. 比对基准快照与当前灰度环境的测试结果，容差时延设置为 50%
$ rupost compare-snapshots baseline-v1.json --env gray --latency-tolerance 0.5

Snapshot Comparison Report:
  Baseline: v1-production (2026-06-05 20:00:00)
  Current:  v2-gray (2026-06-05 23:15:00)

┌──────────────────┬────────────┬────────────┬─────────────┐
│ Endpoint         │ Baseline   │ Gray (Run) │ Status      │
├──────────────────┼────────────┼────────────┼─────────────┤
│ GET /api/users   │ 200 (45ms) │ 200 (98ms) │ ⚠ SLOW (+117%)│
│ POST /api/pay    │ 201 (120ms)│ 400 (15ms) │ ✗ SCHEMA    │
└──────────────────┴────────────┴────────────┴─────────────┘

[Schema Diff: POST /api/pay]
  Expected Field: "transaction_id" (String)
  Actual: Missing!
  
[Performance degradation detected]
  GET /api/users latency increased by 117% (exceeds threshold 50%).
```

---

### 📖 US 5：WebSocket 长连接双向交互时序帧重放 (实时应用排障场景)
> **场景描述**：作为实时应用开发，针对 WebSocket 或 MQTT 连接中断问题，我希望录制一段长连接通信（带高精度消息递增时戳、方向及控制帧），并在本地沙箱中一比一时序重放消息流，观测本地状态机的响应。

#### 💻 CLI 交互设计流：
```bash
# 1. 录制线上 WebSocket 会话 2 分钟
$ rupost ws record wss://prod.example.com/chat --duration 2m --output ws-session.json

[WS Recording]
  Captured: 45 frames (30 Inbound, 15 Outbound, 2 Ping/Pong)

# 2. 本地沙箱时序回放
$ rupost ws replay ws-session.json --target ws://localhost:8080/chat --speed 1.0

[WS Replayer]
  [00:00.00] Sending Outbound Text: "Hello"
  [00:00.45] Received Inbound Text: "Welcome, user_1" (450ms latency)
  [00:15.00] Sending Outbound Ping
  [00:15.02] Received Inbound Pong (20ms latency)
  [00:45.00] ✗ Connection closed by local server. Status code: 1008 (Policy Violation).
  
  [Analysis] Local server disconnected after receiving malformed frames in session.
```

---

### 📖 US 6：本地开发内循环高频调试与 Schema 自动校验 (开发阶段场景)
> **场景描述**：作为全栈开发，在日常开发迭代中，我希望通过轻量级的 Markdown 文档定义 API，一键执行本地测试。在修改后端代码后，能够随时加载本地 `.env` 的快速局部覆盖，并以极致清晰、高亮易读的红绿 Diff 格式在终端直观看到 Schema 或字段值的偏离，快速进行开发自测。

#### 💻 CLI 交互设计流：
```bash
# 1. 在本地开发过程中，热重载本地变量并运行测试，输出极简 Diff
$ rupost test api-docs.md --env dev --watch --diff-format pretty

[Watcher] Watching api-docs.md and .env...
[Run] File modified. Re-executing...

✗ Request #2: 获取配置信息 - FAILED
  Expected: status == 200
  Actual:   status == 200 (Passed)
  
  [Response Body Diff]
  {
    "app_name": "Rupost-Dev",
-   "version": "0.1.0",
+   "version_code": 100,  <-- [CHANGED]
-   "features": ["auth", "mock"]
+   "features": null      <-- [DEVIATION]
  }
```

---

## 3. 系统数据结构与 API 深度走查 (Walkthrough)

依据上述 6 个典型场景的闭环使用流程，我们对现有的规划设计（如 `Diagnostics`、`Snapshot`、`LogFilter`、`LogAdapter` 等）以及 `src/variable` 的现有代码进行深度代码走查，定位出以下 **12 项核心技术 Gap**：

### 🔍 现有规划 vs 场景需求 Gap 矩阵

| 走查组件 | 现有/规划设计 | 场景需求 (场景 1-6) | 关键差距 (Gap) 与隐患 |
| :--- | :--- | :--- | :--- |
| **Diagnostics** | 基础时间字段 (`dns_lookup_time` 等)。 | **场景 1 (微服务)**: 链路及依赖健康指标。<br>**场景 4 (回归)**: 历史时延标准偏差与退化百分比。 | 1. 缺乏对多 Pod 负载均衡路由、容器主机元数据的记录。<br>2. 缺乏性能退化率的方差/偏差统计指标。 |
| **Snapshot** | `request` 和 `response` 分离结构，仅支持 HTTP 明文存储。 | **场景 2 (技术支持)**: 物理脱敏与快照加密。<br>**场景 5 (WS)**: 时序流全双工多帧交互存储。 | 3. 没有敏感信息混淆/哈希掩码策略，直接落盘有严重合规风险。<br>4. 不支持长连接（时序帧、状态帧）的序列化表示，无法记录 WebSocket 会话。 |
| **LogFilter** | 简单的 `trace_id`、`level`、`keywords` 过滤。 | **场景 1 (微服务)**: 多行 Java Stacktrace、Go Panic 日志合并显示。 | 5. 缺乏多行日志合并规则（Multiline Pattern）支持，抓取时会把异常栈切碎。<br>6. 缺乏对不同 Pod / Service 日志流的时间戳对齐。 |
| **LogAdapter** | `fetch_logs` 一次性拉取 API。 | **场景 1 (微服务)**: 海量日志的流式传输、远程 SSH/K8s 鉴权密钥隔离。 | 7. 缺乏流式 (Chunked/Stream) 传输机制，高频高负载时内存极易溢出。<br>8. 鉴权信息直接写在配置文件，缺乏与系统凭证管理器（OS Keychain）或局部环境变量的解耦。 |
| **MockMatcher** | 精确的 URL + Method 匹配。 | **场景 3 (前端)**: 模糊路径、Query 参数变化匹配、特定用户多路分支 Mock。 | 9. 不支持 Regex 通配符匹配及 Query 参数通配规则。<br>10. 缺乏 `MockVariant`（条件变体）数据结构设计，无法对同一接口提供不同输入的 Mock 响应。 |
| **ReplayEngine** | 恢复上下文并重新 execute。 | **场景 2 (技术支持)**: 动态 Token 自动重写、局部变量注入重定向。 | 11. 缺少发包前的动态修改钩子（Interceptor Hook），无法局部重写快照中的过期凭证。 |
| **VariableConfig** | `environments` 多环境配置静态加载。 | **场景 6 (本地开发)**: `.env` 热重载、局部快速环境变量覆盖。 | 12. 缺乏本地文件变化监听（File Watcher）热重载能力，且对开发常用的 `.env` 支持不足。 |

---

## 4. 针对走查问题的 Clean Architecture 架构演进与修改建议

为了解决上述走查中发现的 12 项 Gap，我们在 **Clean Architecture** 架构体系（Interface 层、UseCase 层、Domain 层、Infrastructure 层）的指导下，提出如下具体的系统数据结构与接口演进设计方案：

### 🛠️ 建议 1：解耦 HTTP 局限，重构 Snapshot 为多态会话快照 (解决 Gap 3, 4)
*   **设计重构**：将 Domain 层的 `Snapshot` 改造为多态 Enum，支持单次 HTTP 与流式 WebSocket / gRPC 时序会话。
*   **代码演进示意**：
```rust
// src/debug_mode/snapshot/domain/model.rs

pub enum SnapshotContent {
    Http(HttpExchange),
    WebSocket(WsSessionExchange),
}

pub struct Snapshot {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub environment: String,
    pub content: SnapshotContent,
    pub is_masked: bool,
    pub encryption_algo: Option<String>,
}

pub struct WsSessionExchange {
    /// 一段时间内的所有帧
    pub frames: Vec<WsFrameSnapshot>,
}

pub struct WsFrameSnapshot {
    pub relative_ms: u64,           // 相对于连接建立时的毫秒偏移
    pub direction: FrameDirection,  // Inbound / Outbound
    pub frame_type: WsFrameType,    // Text, Binary, Ping, Pong, Close
    pub payload: Vec<u8>,           // 帧数据
}
```

### 🛠️ 建议 2：在数据落盘前引入 DataMasker 混淆切面与加密层 (解决 Gap 3, 8)
*   **设计重构**：在 Infrastructure 层的 `SnapshotRepo` 写入数据前，强制挂载 `DataMasker`，并利用对称加密（AES-GCM-256）保护快照包。支持 SSH 密钥等敏感项从 Keychain / 环境变量中提取。
*   **代码演进示意**：
```rust
// src/debug_mode/snapshot/infrastructure/masker.rs

pub trait DataMasker: Send + Sync {
    fn mask_headers(&self, headers: &mut HashMap<String, String>, rules: &[String]);
    fn mask_body(&self, body: &mut Option<String>, rules: &[String]);
}

pub struct SnapshotEncryptor;

impl SnapshotEncryptor {
    pub fn encrypt(data: &[u8], key: &[u8]) -> Result<Vec<u8>, EncryptionError> {
        // 使用 AEAD (如 AES-GCM) 算法对快照字节流进行加密
        unimplemented!()
    }
}
```

### 🛠️ 建议 3：日志流式拉取（Streaming）与多行合并过滤器 (解决 Gap 5, 6, 7)
*   **设计重构**：改造 `LogAdapter` 接口，用异步 Rust 的 `Stream` 替换单一的 `Result<Vec<LogEntry>>` 从而支持流式大日志过滤；在 UseCase 层引入 `MultilineCombiner` 模块处理崩溃栈。
*   **代码演进示意**：
```rust
// src/debug_mode/logs/usecase/combiner.rs

use futures_util::Stream;
use std::pin::Pin;

#[async_trait]
pub trait LogAdapter: Send + Sync {
    /// 返回异步日志帧流，解耦内存瓶颈
    async fn fetch_logs_stream(
        &self, 
        filter: &LogFilter
    ) -> Result<Pin<Box<dyn Stream<Item = Result<LogEntry>> + Send>>, LogError>;
}

pub struct MultilineCombiner {
    /// 判定多行日志开始的正则表达式（如匹配时间戳 "^\d{4}-\d{2}-\d{2}"）
    start_pattern: Regex,
}

impl MultilineCombiner {
    pub fn combine(&self, raw_lines: Vec<String>) -> Vec<LogEntry> {
        // 将未配对的 Stacktrace 多行文本聚合成单个 LogEntry 的 message 字段
        unimplemented!()
    }
}
```

### 🛠️ 建议 4：基于通配符的模糊 Mock 引擎与分支变体设计 (解决 Gap 9, 10)
*   **设计重构**：在 Mock Server 侧，引入 `MockRoute` 的 Trie 树路由匹配，支持路径参数（Path Variable）和通配符匹配。同时引入 `MockVariant` 评估请求字段分发。
*   **代码演进示意**：
```rust
// src/debug_mode/mock/domain/matcher.rs

pub struct MockRoute {
    pub path_pattern: String,      // 例如: "/api/users/:id/billing"
    pub method: String,
    pub query_wildcards: HashMap<String, String>,
    pub variants: Vec<MockVariant>,
}

pub struct MockVariant {
    pub condition: VariantCondition, // 判定条件，例如: "body.username == 'admin'"
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub response_body: String,
}
```

### 🛠️ 建议 5：重构 ReplayEngine 引入拦截器管道（Interceptor Pipeline）(解决 Gap 11)
*   **设计重构**：为了让保存的快照能重现于不同环境，必须能够在执行前动态修正/注入参数。我们需要设计一个 Pipeline，类似于 HTTP 客户端的拦截器中间件。
*   **代码演进示意**：
```rust
// src/debug_mode/replay/usecase/engine.rs

#[async_trait]
pub trait ReplayInterceptor: Send + Sync {
    async fn intercept(&self, req: &mut RequestSnapshot, ctx: &VariableContext);
}

pub struct ReplayEngine {
    interceptors: Vec<Box<dyn ReplayInterceptor>>,
}

impl ReplayEngine {
    pub async fn replay(&self, snapshot: &Snapshot, env_ctx: &VariableContext) -> Result<Response, ReplayError> {
        let mut req_snap = snapshot.content.as_http_request().clone();
        
        // 依次执行拦截器，动态重写凭证、API Base URL 等
        for interceptor in &self.interceptors {
            interceptor.intercept(&mut req_snap, env_ctx).await;
        }
        
        // 执行底层 HTTP/WS 发包
        unimplemented!()
    }
}
```

### 🛠️ 建议 6：添加配置文件热加载与 .env 级联覆盖机制 (解决 Gap 12)
*   **设计重构**：利用 `notify` 监听 `rupost.toml` 和本地 `.env` 文件的变化事件，触发 `ConfigLoader` 的热加载；重新设计变量覆盖优先级树，最底层支持级联式 `.env` 局部加载。
*   **代码演进示意**：
```rust
// src/variable/config.rs 演进建议

pub struct HotReloadConfigLoader {
    config_path: PathBuf,
    env_file_path: Option<PathBuf>,
}

impl HotReloadConfigLoader {
    pub fn start_watching<F>(&self, on_change: F) 
    where 
        F: Fn(VariableConfig) + Send + 'static 
    {
        // 绑定 notify 管道事件，变更时自动触发 on_change 重新生成 VariableContext 并在 TUI/CLI 中更新
        unimplemented!()
    }
}
```

---

## 5. 总结与实施建议

本走查报告通过 6 个紧贴开发生命周期（从本地开发内循环、多协议 WebSocket 调试、前端 Mock 联调，到灰度发布回归测试、生产微服务 500 级故障定位）的真实使用场景，清晰印证了：
*   **环境隔离、状态脱敏和多协议流式重放**是生产调试功能的生命线。
*   现有的简单 HTTP 单次拉取设计在面临大规模企业级排障时存在安全漏洞（数据泄露、密钥明文暴露）以及性能瓶颈（大日志内存耗尽、Mock 精确匹配过窄）。

我们建议，在未来的 3.0 开发中，依据本报告第 4 节中的**多态会话快照**、**流式日志传输**、**动态 Mock 路由 Trie 树**以及**拦截器 Pipeline** 方案，在 Domain 层和 UseCase 层进行优先的代码重构，确保 Rupost 的架构在保持极致简洁的同时，具备业界顶级的生产调试竞争力。
