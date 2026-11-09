# 02 - 离线产物黑匣子契约与插件深度挖掘规范

- **状态**: Approved
- **归属任务**: `cross-scenario-engine-and-plugins`
- **核心原则**: **“现场无损记录，本地深度挖掘”** —— 基础功能生成的产物必须是高保真的数据黑匣子，插件在离线状态下依然能挖掘出全景信息。

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
│  • 原始响应体与完整响应头                              │
│  • 微秒级网络时序 (DNS/TCP/TLS/TTFB)                    │
│  • 物理 Socket 诊断指纹 (远端 IP, TLS 算法, 证书链)    │
│  • 告警与环境指纹 (缺失插件记录, 未定义变量)           │
└──────────────────────────┬─────────────────────────────┘
                           │
                           ▼ (拷回开发环境，供富插件二次消费)
┌────────────────────────────────────────────────────────┐
│          开发环境: 插件消费与深层次信息挖掘            │
├──────────────────────────┬─────────────────────────────┤
│ 1. AI 智能根因分诊插件   │ • 读取 dump.json，分析生产网关 504 错误与 Envoy 响应片段，秒级定位是内网 DNS 抖动还是业务死锁 |
├──────────────────────────┼─────────────────────────────┤
│ 2. 性能火焰图与时延插件  │ • 提取微秒级 TimingWaterfall，渲染全链路交互式火焰图与慢请求热力图                           |
├──────────────────────────┼─────────────────────────────┤
│ 3. 数据安全与泄密审计    │ • 扫描生产真实响应，审计是否泄露了手机号、明文 Token、内部私有 IP                             |
├──────────────────────────┼─────────────────────────────┤
│ 4. 契约漂移与 Diff 插件  │ • 将生产 dump.json 与开发环境快照进行结构化比对，秒级捕获字段类型漂移或字段被意外删除       |
└──────────────────────────┴─────────────────────────────┘
```

---

## 2. 离线产物数据结构规范 (`src/history/model.rs`)

基础功能在开启 `--save-snapshot <FILE>` 时，保证将每个测试请求序列化为以下**强类型结构（Schema Version 1.0）**：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionSnapshotFile {
    /// 契约版本
    pub schema_version: String, // 恒定 "1.0"
    /// 执行时间戳 (ISO 8601 UTC)
    pub generated_at: String,
    /// 运行环境标识 (可选，由 --env 或系统探测提供)
    pub environment: Option<String>,
    /// 运行指纹 (OS, Arch, rupost version)
    pub runner_fingerprint: RunnerFingerprint,
    /// 完整的测试条目列表
    pub entries: Vec<ExecutionSnapshotEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionSnapshotEntry {
    pub request_number: usize,
    pub name: Option<String>,
    pub file_path: String,
    pub success: bool,
    pub status: Option<u16>,
    pub duration_us: u64, // 微秒级总耗时

    /// 1. 原始发包请求镜像 (经过所有加签和变量解析后的最终形态)
    pub request: SnapshotRequest,

    /// 2. 完整原始响应镜像
    pub response: Option<SnapshotResponse>,

    /// 3. 微秒级细粒度网络时延拆解 (核心性能指标)
    pub timing_waterfall: Option<TimingWaterfallUs>,

    /// 4. 底层物理网络连接诊断指纹
    pub socket_diagnostics: Option<SocketDiagnostics>,

    /// 5. 规则断言结果明细
    pub assertions: Vec<SnapshotAssertion>,

    /// 6. 运行期非致命告警 (如降级说明、未定义变量等)
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimingWaterfallUs {
    pub dns_lookup_us: u64,
    pub tcp_connect_us: u64,
    pub tls_handshake_us: Option<u64>,
    pub ttfb_us: u64,
    pub transfer_us: u64,
    pub total_us: u64,
}

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

---

## 3. 插件对离线产物的消费 API (Mining Consumer Protocol)

外部插件或开发环境扩展工具可通过以下标准化形态消费该产物：

### 3.1 CLI 管道直接挂载消费
```bash
# 开发环境中，插件直接基于现场 dump.json 运行分析：
rupost-plugin-flamegraph --input /tmp/incident-dump.json --output ./report.html
rupost-plugin-security-audit --input /tmp/incident-dump.json --strict
```

### 3.2 官方 AI 归因深度挖掘集成
`rupost` 内部已深度对齐该消费模式：
```bash
# 即使 incident-dump.json 是在没有 Python、没有外网的机房生产的，
# 拷贝到有大模型 Key 的开发机上，直接执行离线分诊与根因归因：
rupost replay ./incident-dump.json --explain
rupost ai triage --report-file ./incident-dump.json
```

---

## 4. 保真与脱敏的平衡控制 (Masking Control)

在调试环境中导出数据时，面临安全合规问题。`rupost` 基础功能提供明确的脱敏开关契约：
* **默认形态 (`--save-snapshot run.json`)**：
  保持 **100% 原始字节高保真**（不篡改 Header、不截断 Body），确保开发环境中的 Diff 插件和加签校验插件能够执行精准的字节级验算；
* **合规掩码形态 (`--save-snapshot run.json --mask-secrets`)**：
  若现场安全策略要求机密不离机，开启 `--mask-secrets`，引擎自动应用 `crate::ai::redactor`，将 `Authorization`、Cookie、密码字段掩码为 `***` 后再写盘。
