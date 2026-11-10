# 02 - 离线产物黑匣子契约与插件深度挖掘规范

- **状态**: Draft (v2.1.0 — 快照契约唯一权威 SSOT；评审通过后回标 Approved)
- **归属任务**: `cross-scenario-engine-and-plugins`
- **权威声明**: **本文件是快照产物 Schema 的唯一事实来源**。04 的测试代码、05 的相关表述、06 的实现任务均以本 Schema 为准。
- **核心原则**: **"现场无损记录，本地深度挖掘"** —— 基础功能生成的产物必须是高保真的数据黑匣子，插件在离线状态下依然能挖掘出全景信息。
- **范围标签约定**: `[已有]` = 现状存在（附 file:line 锚点）；`[待建 M<N>]` = 目标态，归属 06 的 Milestone N。

---

## 1. 产物设计架构与数据流向

```
┌────────────────────────────────────────────────────────┐
│   现场 / 调试环境 (无外部插件，即拷即走，极简执行)     │
│   rupost test suite.http --save-snapshot dump.json     │
└──────────────────────────┬─────────────────────────────┘
                           │
                           ▼ (单二进制原生序列化，0 运行时损失)
┌────────────────────────────────────────────────────────┐
│            统一数据黑匣子: dump.json (Schema v1.0)      │
│  • 原始请求与网络发包字节镜像                          │
│  • 原始响应体与完整响应头 (256KB 防 OOM 截断 + SHA256)  │
│  • 微秒级网络时序 (DNS/TCP/TLS/TTFB) [待建 M1]          │
│  • 物理 Socket 诊断指纹 (远端 IP, TLS 算法, 证书链)     │
│  • 顶级错误详情 (故障 100% 录制，绝不静默吞没)          │
│  • 三态判定与降级标记 (ExecutionOutcome) [已有设计]     │
│  • 告警与环境指纹 (缺失插件记录, 未定义变量)            │
└──────────────────────────┬─────────────────────────────┘
                           │
                           ▼ (拷回开发环境，供富插件二次消费)
┌────────────────────────────────────────────────────────┐
│          开发环境: 插件消费与深层次信息挖掘            │
├──────────────────────────┬─────────────────────────────┤
│ 1. AI 智能根因分诊插件   │ • 读取 dump.json，分析生产网关 504 错误与 Envoy 响应片段，秒级定位是内网 DNS 抖动还是业务死锁 |
├──────────────────────────┼─────────────────────────────┤
│ 2. 性能火焰图与时延插件  │ • 提取微秒级 TimingWaterfall，渲染全链路交互式火焰图与慢请求热力图                           │
├──────────────────────────┼─────────────────────────────┤
│ 3. 数据安全与泄密审计    │ • 扫描生产真实响应，审计是否泄露了手机号、明文 Token、内部私有 IP                             │
├──────────────────────────┼─────────────────────────────┤
│ 4. 契约漂移与 Diff 插件  │ • 将生产 dump.json 与开发环境快照进行结构化比对，秒级捕获字段类型漂移或字段被意外删除       │
└──────────────────────────┴─────────────────────────────┘
```

---

## 2. 离线产物数据结构规范（Schema v1.0，唯一权威定义）

### 2.0 现状基线与迁移映射 `[已有 → 待建 M1]`

**现状**（`src/history/model.rs`）：快照实际结构为 `SnapshotSuite`(:108，`version: u32 = 1`) / `SnapshotEntry`(:73，仅 `id/request/response/error` 四字段) / `RequestSnapshot`(:31) / `ResponseSnapshot`(:123) / `ExecutionErrorDetail`(:60)。快照**不含**时序、告警、Socket 诊断；时序数据现只存在于 diagnose 命令与 web 实时报告（`src/report/types.rs:112` `TimingWaterfall`，全毫秒字段）。本节 Schema 为**待建目标态**，迁移任务归属 06 M1（重构 `save_batch_snapshot` 写入链路 `src/history/storage.rs:304`）。

| 现状 | 目标 |
| :--- | :--- |
| `SnapshotSuite` | `ExecutionSnapshotFile` |
| `SnapshotEntry` | `ExecutionSnapshotEntry` |
| `RequestSnapshot` / `ResponseSnapshot` | `SnapshotRequest` / `SnapshotResponse` |
| `ExecutionErrorDetail` | **保留复用**（定义见 §2.5，快照不新造平行类型） |
| （无） | `TimingWaterfallUs` / `SocketDiagnostics` / `masking` 字段 |

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionSnapshotFile {
    /// 契约版本，当前 "1.0"。演进规则见 §2.6
    pub schema_version: String,
    /// 执行时间戳 (ISO 8601 UTC)
    pub generated_at: String,
    /// 运行环境标识 (可选，由 --env 或系统探测提供)
    pub environment: Option<String>,
    /// 运行指纹 (OS, Arch, rupost version)
    pub runner_fingerprint: RunnerFingerprint,
    /// 脱敏形态标记（消费者据此判断产物敏感性，见 §4）
    pub masking: MaskingMode,
    /// 完整的测试条目列表
    pub entries: Vec<ExecutionSnapshotEntry>,
}

/// MaskingMode：serde 小写序列化（"none" | "secrets"）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MaskingMode {
    /// 100% 原始字节保真
    None,
    /// 已应用 --mask-secrets 掩码
    Secrets,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunnerFingerprint {
    pub os: String,
    pub arch: String,
    pub rupost_version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionOutcome {
    /// 全部插件与断言 100% 正常通过
    Pass,
    /// 发生可选插件缺失降级跳过，但基础 HTTP 请求与断言通过（03 §10.5 三态判定）
    DegradedPass,
    /// 断言失败、网络异常或未授权插件缺失
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionSnapshotEntry {
    pub request_number: usize,
    pub name: Option<String>,
    pub file_path: String,
    pub success: bool,
    pub outcome: ExecutionOutcome,
    pub status: Option<u16>,
    /// 纯物理网络微秒级总耗时（严格隔离插件冷启动耗时）
    pub duration_us: u64,
    /// 外部插件管道总耗时（03 §8.3 时延隔离；无插件调用时为 None）
    pub plugin_duration_us: Option<u64>,

    /// 1. 原始发包请求镜像 (经过所有加签和变量解析后的最终形态)
    pub request: SnapshotRequest,

    /// 2. 完整原始响应镜像 (256KB 防 OOM 截断保护与 SHA-256 摘要)
    pub response: Option<SnapshotResponse>,

    /// 3. 网络/协议/插件执行错误详情 (确保 DNS 失败或连接超时 100% 留存现场)
    pub error: Option<ExecutionErrorDetail>,

    /// 4. 微秒级细粒度网络时延拆解 (核心性能指标；连接复用时部分字段为 None)
    pub timing_waterfall: Option<TimingWaterfallUs>,

    /// 5. 底层物理网络连接诊断指纹
    pub socket_diagnostics: Option<SocketDiagnostics>,

    /// 6. 规则断言结果明细
    pub assertions: Vec<SnapshotAssertion>,

    /// 7. 运行期非致命告警 (如降级说明、未定义变量、插件 stderr 摘要等)
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// 最终发送的 Body（含插件变形/加签后形态；无 Body 时为 None）
    pub body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
    /// 若响应体超过 --max-snapshot-body-bytes (默认 256KB)，标记为 true
    pub body_truncated: bool,
    /// 截断前完整原始响应体的 SHA-256 校验和，供离线比对验真
    pub body_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotAssertion {
    pub expression: String,
    pub passed: bool,
    pub actual: Option<String>,
    pub expected: Option<String>,
    pub message: Option<String>,
}

/// 连接复用语义：None = 该阶段未发生（keep-alive 复用既有连接）；
/// Some(0) 仅在实际发生且耗时向下取整为 0 时出现。
/// HTTP/2 多路复用下，socket_diagnostics 描述的是共享连接，非单请求独占。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimingWaterfallUs {
    pub dns_lookup_us: Option<u64>,
    pub tcp_connect_us: Option<u64>,
    pub tls_handshake_us: Option<u64>,
    pub ttfb_us: u64,
    pub transfer_us: u64,
    /// 纯网络侧总耗时，恒等于 entry.duration_us 的口径（二者差值 = 插件耗时，已由
    /// plugin_duration_us 单列承载）
    pub total_us: u64,
}

/// 证书字段为 best-effort：reqwest+rustls 公开 API 不暴露对端证书链，
/// 需自定义 connector 埋点；未采集时为 None，绝不阻塞主流程。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocketDiagnostics {
    pub remote_ip: String,
    pub remote_port: u16,
    pub http_version: String, // e.g. "HTTP/1.1" | "HTTP/2"
    pub tls_cipher_suite: Option<String>,
    pub tls_version: Option<String>,
    pub cert_days_remaining: Option<i64>,
    pub cert_issuer: Option<String>,
}
```

### 2.5 `ExecutionErrorDetail`（复用现有类型）

沿用 `src/history/model.rs:60` 现有定义（`kind` / `message` / `duration_ms` / `occurred_at`）。快照契约不重复定义平行结构（DRY/SSOT）。

### 2.6 演进策略（消费者兼容契约）

1. **只允许追加字段，禁止修改既有字段语义、禁止删除字段**；
2. 消费者（插件与 miner）**必须容忍未知字段**（serde 默认行为，写入契约）；
3. 破坏性变更（删字段、改语义）必须递增 `schema_version` 并在 CHANGELOG 登记；
4. 消费者按 `schema_version` 主版本兼容判断（`"1.x"` 均可消费）。

> v2.0.0 注释"恒定 1.0"废除——版本字段被写死等于废除演进机制。

### 2.7 安全不变量（硬约束）

* **`plugin_args` 永不序列化进快照/报告**——插件指令参数（含内核解析后的 `env.*` 值）只经 Stdio IPC 传给插件进程（03 §8），绝不落盘；
* 快照文件落盘权限强制 `0600`（与 umask 无关；多用户堡垒机的 /tmp 上默认 0644 泄露不可接受）；
* `masking` 字段必须如实反映产物形态；消费者可据此拒绝或降级字节级验签类分析（§4）。

---

## 3. 插件对离线产物的消费 API (Mining Consumer Protocol)

### 3.1 CLI 管道直接挂载消费（外部 miner）

```bash
# 开发环境中，插件直接基于现场 dump.json 运行分析：
rupost-plugin-flamegraph --input /tmp/incident-dump.json --output ./report.html
rupost-plugin-security-audit --input /tmp/incident-dump.json --strict
```

外部 miner 调用协议：`<可执行文件> --input <snapshot.json> [options]`，退出码穿透。注册表 `miners` 表的路由化调用入口为 **P2**（03 §3）；MVP 内外部 miner 经 CLI 直挂载消费，不依赖内核 miners 路由。

### 3.2 内置离线挖掘入口

```bash
# replay 场景重放与解释 [已有]（src/cli/commands/replay.rs:27，--explain 为别名 ai-explain）
rupost replay ./incident-dump.json --explain

# AI 归因分诊消费快照 [待建 M5]：首个内置 miner 消费者，
# 复用 crates/rupost-ai 的 FailureAttributor 适配 SnapshotEntry（05 ADR-D9）
rupost ai triage --snapshot ./incident-dump.json
```

> 拼写裁决：`trace analyze` 表述废除（无实现出处，见 05 §1.4 修订）；`ai triage` 现状消费 `test --report-file` 产出的报告 JSON 且 `report_file` 为位置参数（`src/cli/commands/ai.rs:31`，`--report-file` 长旗标写法会被 clap 拒绝）；`--snapshot` 为新增形态，二者并存。

### 3.3 消费前校验（不可信输入防线）

离线 snapshot.json 视为**不可信输入**：
* 消费前强制 schema 校验（未知字段容忍 + 必填字段存在性）与体积上限（默认 64MB，超限拒绝并报错）；
* 所有来自响应体的字符串进入报告时必须做 XML/HTML 转义（`strip-ansi-escapes` 仅剥离 ANSI，不覆盖标记语言注入）——生态插件开发契约。

---

## 4. 保真与脱敏的平衡控制 (Masking Control)

在调试环境中导出数据时，面临安全合规问题。裁决如下：

* **默认形态 (`--save-snapshot run.json`)**：
  内容保持 **100% 原始字节高保真**（不篡改 Header、不截断 Body），确保开发环境中的 Diff 插件和加签校验插件能够执行精准的字节级验算；**但文件权限强制 `0600`**。
* **合规掩码形态 (`--save-snapshot run.json --mask-secrets`)**：
  开启 `--mask-secrets`，引擎在**变量解析之后**、写盘之前应用脱敏 SSOT（`src/ai/redactor.rs` `[已有]`），掩码占位符 `[REDACTED]`，并置 `masking: "secrets"`。
* **现场建议**：生产现场导出建议携带 `--mask-secrets`（文档级引导；默认值保持保真以支撑验签/Diff 用例）。

**掩码覆盖面（表驱动实现 + 表驱动单测锁定）**：

| 类别 | 覆盖项 |
| :--- | :--- |
| 请求头 | `Authorization*`、`Cookie`；名称含 `sign/token/key/secret` 的自定义签名头（如 `X-Sign`） |
| 响应头 | `Set-Cookie` |
| URL | query 参数名匹配 `token/access_token/sign/sig/key/secret` |
| Body | JSON 键名递归匹配 `password/passwd/phone/mobile/id_card/secret/token/access_token` |

* **掩码模式下的验签降级**：`masking: "secrets"` 时，加签校验/Diff 类插件的字节级验算结论 MUST 附带"数据不完整"警告（§2 `MaskingMode` 消费契约）；
* 掩码发生在**变量解析之后**（否则 `{{DEBUG_FALLBACK_TOKEN}}` 解析值漏网）。
