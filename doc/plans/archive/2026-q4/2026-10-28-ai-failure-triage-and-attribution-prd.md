# RuPost AI 失败智能处理 PRD：断言就地归因 (E2) 与 CI 失败聚类 (E5) (v1.1)

> **创建日期**: 2026-10-27（v1.1 经子代理红队审查与主代理代码实证修订）  
> **文档定位**: 遵循 [7-Stage 研发流程规范](file:///Users/zsyzzx/project/rust/rupost/doc/dev/development_workflow.md) Stage 0 (PRD) & Stage 1/2 (架构与关键代码)  
> **父纲与上游**: [AI 总体规划](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-27-ai-master-roadmap.md)（B0–B4 宪法条款）、[AI 切入点补充分析](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-27-ai-entrypoints-supplement.md)、[P0 上下文断言与零配置 PRD](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-27-ai-context-assert-and-zero-config-prd.md)  
> **兜底分级声明（准入强制条款）**:
> - **B0 结构兜底**：`src/runner/` 核心测试执行引擎对 `rupost-ai` 保持**零依赖**；E2/E5 均在执行完毕后的外层报告/CLI 适配层（`src/ai/`）消费测试结果。
> - **E2 (`rupost test --explain`)**：**B1 就地增强型**——基线为确定性规则版 `[Possible Causes]`；AI 可用时追加 LLM 根因诊断；AI 缺失/故障时输出单行黄色 `[!]` 提示并退回规则版基线，**退出码严格由测试本身决定（失败仍为 exit 1）**。
> - **E5 (`rupost ai triage`)**：**B1 批量增强型**——五阶段管线中的 B 阶段为**确定性签名分组基线**；AI 缺失/故障时输出确定性分组报告 + 黄色 `[!]` 警告并以 **exit 0** 正常结束。

---

## 1. 背景、痛点与竞品坐标

### 1.1 从“写测试”到“排查问题”的战场转移
1. **单接口调试场景（开发者，旅程 S3）**：断言失败后，终端仅输出 `status: expected 200, got 401` 或原始响应。开发者需手动比对 actual/expected、查 Header 与前置变量，平均耗费 **10~20 分钟**。
2. **CI 批量回归场景（QA / Tech Lead，旅程 S6）**：CI 跑批一夜红了 50+ 条，终端平铺 50 条错误。人工逐条排查区分“同一网关根因”还是“多个独立 Bug”需 **1~2 小时**。

### 1.2 竞品空白度
- **终端派（Hurl / Posting / slumber / ATAC / Bruno CLI）**：0% 覆盖，失败仅输出原始 diff。
- **云端平台派（Postman Postbot / Apidog）**：Postbot 局限于 GUI 弹窗单点对话修脚本，其 CLI（Newman）无任何就地归因与离线聚类能力。
- **企业级 CI 平台（Datadog CI / Launchable）**：虽支持聚类，但依赖昂贵重型 SaaS 且脱离 HTTP/API 拓扑语义。

---

## 2. 前置条件与代码基建核查（Code Reality vs Requirements）

经主代理与红队子代理逐行核查代码库，落地本 PRD 前必须正视并解决以下 **5 项真实代码现状差异**：

| # | 现状事实（Code Reality） | 对 E2 / E5 的影响 | 解决策略（前置/同步交付） |
| :- | :--- | :--- | :--- |
| **P-1** | `src/main.rs` 因内联 `run_ai_assert` 已膨胀至 **180 行**（突破 `AGENTS.md §6` 的 150 行红线） | 继续在 `main.rs` 堆叠 AI 命令将导致入口层严重腐化 | 建立 `src/ai/mod.rs`，将 `run_ai_assert` 与新命令全部下沉至库层，`main.rs` 恢复 ~120 行 |
| **P-2** | P0 地基 `src/ai/{config,redactor}.rs` 尚未落地；但 `src/exporter/redact.rs` 已有成熟 Header/URL 脱敏 | 若在 `rupost-ai` 重复写脱敏器会违反 DRY 与 B3 边界 | 在主包提炼共享脱敏与截断器（复用 `exporter::redact` + 新增 JSON 敏感键掩码与截断），统一走 B4 配置链 |
| **P-3** | `TestReporter::report_json`（`src/reporter.rs:246`）手写匿名 `json!`，无反序列化结构体，且**丢弃了 `expected`/`actual` 与响应体片段** | `rupost ai triage <report.json>` 无法强类型解析报告，且看不到 4xx/5xx 响应体与断言期望值 | 将 JSON 报告升级为强类型可序列化/反序列化的 `JsonFileReport` / `JsonTestReport`，补充 `expected`、`actual` 与脱敏后的失败响应体片段 `response_body_snippet` |
| **P-4** | `--report` 目前仅接受 `"terminal"` 或 `"json"`（输出到 stdout），不支持直接写文件路径 | 用户跑 `rupost test --report target/report.json` 会被当成非 json 分支打印终端文本 | 新增 `--report-file <FILE>` 落盘 JSON 报告；同时支持 `rupost test --triage` 直接在内存中对 `batch_results` 聚类 |
| **P-5** | `TestResult` 仅有 1-based `request_number` 和 `name`，无文件物理行号；网络错误时 `status` 为 `None` | E2/E5 不能假设 `status` 必存在，也不能输出虚构的文件行号 | 标识格式统一为 `<file_path>#<request_number> (<name>)`；`FailureContext.status_code` 定为 `Option<u16>` 并包含 `error: Option<String>` |

---

## 3. 目标与非目标 (Scope & Boundaries)

### 3.1 目标
1. **E2：断言失败就地归因 (`rupost test --explain`)**
   - 标志命名与 [MVP 汇总计划 R7](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-27-ai-mvp-consolidation-plan.md) 对齐为 `--explain`（同时兼容别名 `--ai-explain` 与环境变量 `RUPOST_AI_EXPLAIN=1`）；
   - 先输出确定性规则版 `[Possible Causes]`（B1 基线），再追加 AI 根因诊断；
   - **风暴熔断保护（Circuit Breaker）**：单次运行最多对前 `MAX_EXPLAIN_FAILURES = 3` 条（按错误签名去重）调用 LLM，超出部分回退规则版并提示使用 `--triage`。
2. **E5：CI 失败聚类 Triage (`rupost ai triage <report.json>` & `rupost test --triage`)**
   - 采用**“确定性签名分桶（阶段 B） + LLM 语义合并归因（阶段 C） + 确定性回填校验（阶段 D）”**三步架构；
   - 保证**用例总数 100% 守恒、零幻觉 ID、Token 消耗恒定为 $O(M)$（$M$ 为签名桶数，上限 15）**；
   - 无 AI 时直接输出阶段 B 的确定性签名分组报告（exit 0）。
3. **零真实 Key 离线 Dogfooding 闭环**
   - 提供 `examples/ai_triage/mock_ai.json`，通过 `rupost mock` 完成端到端验证并接入 `tests/verify_features.sh`。

### 3.2 显式非目标
- **绝不污染执行期判定**：测试成功/失败 100% 由确定性断言决定，AI 仅在执行结束后只读介入。
- **绝不自动改写测试文件**：AI 仅输出诊断建议，不覆写 `.http`/`.md`。
- **绝不让 `crates/rupost-ai` 依赖主包类型**：严格遵守 B3，`crates/rupost-ai` 仅接收纯数据 DTO。

---

## 4. 用户故事与验收标准 (Acceptance Criteria)

### US1：单请求失败就地归因与风暴防护 (E2)
- **Given** 请求返回 `401 Unauthorized` 且断言 `status == 200` 失败
- **When** 运行 `rupost test api.http --explain`（别名为 `--ai-explain`）
- **Then** 终端在该失败请求下方输出规则版提示与 AI 根因诊断：
  ```text
     [Possible Causes (Rule)]
       - HTTP 401 Unauthorized: 请求缺少鉴权头或凭据已失效
     [AI Diagnosis]
       Summary: 认证失败导致后续字段断言未命中
       Causes:
         1. 请求头未携带有效的 Authorization Bearer Token
         2. 前置登录接口捕获的变量未传入当前环境
       Action: 检查前置 login 请求或确认 --env 环境变量配置
  ```
- **And** 若同一批次有 10 条请求失败，仅对前 3 个不同错误签名的失败请求调用 AI，末尾打印：
  `[i] 其余 7 条失败已省略 AI 单点归因，建议使用 --triage 查看批量根因聚类`
- **And** 若未配置 AI 且无本地 Ollama：输出规则版 `[Possible Causes (Rule)]` + 单行黄色警告 `[!] AI 归因已跳过（未检测到可用 AI 配置）`，进程退出码仍为 `1`（反映测试失败）。

### US2：CI 批量失败报告智能聚类 (E5)
- **Given** `rupost test suite/ --report-file target/report.json` 生成了含 42 条失败的报告
- **When** 运行 `rupost ai triage target/report.json`（或直接 `rupost test suite/ --triage`）
- **Then** 输出按影响面降序排列的根因簇简报：
  ```text
  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  RuPost Failure Triage Summary (Total Failed: 42, Clusters: 3)
  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

  [Cluster #1] [InfrastructureTimeout] 网关鉴权服务连接超时 (31 tests)
    Signature : ERROR(请求超时) @ /api/v1/*
    Sample    : tests/order/create.http#2 (create-order)
    Root Cause: /api/v1/* 路由下游响应超时，导致批量请求级联失败。

  [Cluster #2] [ContractMismatch] 响应契约字段变更 (9 tests)
    Signature : HTTP 200 | assert: body.token exists (actual: None)
    Sample    : tests/auth/login.http#1 (user-login)
    Root Cause: 接口返回 200 但响应体为 {"access_token": "..."}，原 token 字段已更名。

  [Cluster #3] [InternalServerError] 服务端内部异常 (2 tests)
    Signature : HTTP 500 @ /api/v1/users/profile
    Sample    : tests/user/profile.http#1 (update-profile)
    Root Cause: 服务端返回 500 Internal Server Error，请排查后端异常日志。
  ```
- **And** 所有簇的 `(N tests)` 之和严格等于 `Total Failed: 42`（100% 计数守恒）。
- **And** 支持 `--json` 输出机器可读 JSON 聚类结果。
- **And** 当无 AI 可用时（B1 降级）：输出黄色警告 `[!] AI 不可用，已输出确定性签名分组结果`，展示按 Signature 聚合的确定性簇，退出码为 `0`。

---

## 5. Clean Architecture 架构设计与五阶段管线

```
┌──────────────────────────────────────────────────────────────────────────┐
│ 1. 核心测试执行层 (src/runner/) —— 【B0 零 AI 依赖】                      │
│    执行请求 → 产出 Vec<(PathBuf, Vec<TestResult>)>                        │
└────────────────────────────────────┬─────────────────────────────────────┘
                                     │
                                     ▼
┌──────────────────────────────────────────────────────────────────────────┐
│ 2. 主包 AI 编排与脱敏层 (src/ai/{mod, config, redactor, explain, triage}) │
│    - A 素材提取: 从 TestResult / JsonFileReport 提取失败项                │
│    - A' 脱敏截断: 复用 exporter::redact + JSON 敏感键掩码 + Body 1KB 截断 │
│    - B 确定性基线:                                                       │
│        * E2: rule_based_causes(&FailureContext) → 规则版原因              │
│        * E5: cluster_by_signature(&[FailureItem]) → Vec<SignatureBucket> │
└────────────────────────────────────┬─────────────────────────────────────┘
                                     │ 纯 DTO (不含 rupost 内部类型)
                                     ▼
┌──────────────────────────────────────────────────────────────────────────┐
│ 3. 独立 AI 领域 Crate (crates/rupost-ai/src/{attribution, triage}.rs)    │
│    - C AI 语义增强: 构造紧凑 Prompt → 调用 AiClient::generate_text       │
│    - D 宽容解析与裁决: 剥离 Markdown 围栏 → 宽容反序列化 → 桶 ID 守恒回填 │
└──────────────────────────────────────────────────────────────────────────┘
```

---

## 6. 核心数据模型与接口契约 (Stage 2)

### 6.1 JSON 报告强类型化 (`src/reporter.rs`)
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonFileReport {
    pub file_path: String,
    pub results: Vec<JsonTestReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonTestReport {
    pub request_number: usize,
    pub name: Option<String>,
    pub method: String,
    pub url: String,
    pub status: Option<u16>,
    pub duration_ms: u128,
    pub success: bool,
    pub error: Option<String>,
    pub skipped: bool,
    pub assertions: Vec<JsonAssertionReport>,
    #[serde(default)]
    pub warnings: Vec<String>,
    /// 仅在失败（!success）时附带脱敏并截断（≤1KB）后的响应体摘要，供离线 triage 诊断
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_body_snippet: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonAssertionReport {
    pub raw: String,
    pub passed: bool,
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual: Option<String>,
}
```

### 6.2 单次失败归因契约 (`crates/rupost-ai/src/attribution.rs`)
```rust
/// 已在主包完成脱敏与截断的单次失败上下文 DTO
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureContext {
    pub test_id: String,
    pub method: String,
    pub url: String,
    pub status_code: Option<u16>,
    pub latency_ms: u64,
    pub error: Option<String>,
    pub request_headers: Vec<(String, String)>,
    pub failed_assertions: Vec<FailedAssertionInfo>,
    pub response_body_snippet: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailedAssertionInfo {
    pub raw_assertion: String,
    pub expected: String,
    pub actual: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributionAdvice {
    pub summary: String,
    #[serde(default)]
    pub probable_causes: Vec<String>,
    #[serde(default)]
    pub suggested_action: Option<String>,
}
```

### 6.3 批量失败聚类契约 (`crates/rupost-ai/src/triage.rs`)
```rust
/// 阶段 B：确定性签名桶（由 Rust 确定性代码生成，确保计数 100% 准确）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureBucket {
    pub bucket_id: usize,
    pub signature: String,
    pub default_category: FailureCategory,
    pub test_ids: Vec<String>,
    pub sample_context: FailureContext,
}

/// 最终输出的失败聚类簇
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureCluster {
    pub id: usize,
    pub title: String,
    pub category: FailureCategory,
    pub signature_summary: String,
    pub affected_tests_count: usize,
    pub representative_test: String,
    pub root_cause_analysis: String,
    pub test_ids: Vec<String>,
}

/// 宽容反序列化的故障分类枚举：任何未知字符串自动归入 Other(String)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum FailureCategory {
    InfrastructureTimeout,
    AuthenticationExpired,
    ContractMismatch,
    InternalServerError,
    AssertionLogicError,
    Other(String),
}
```

---

## 7. 实施步骤与原子提交序列 (Execution Plan)

| 步骤 | 提交信息 | 核心内容 | 验证指标 |
| :---: | :--- | :--- | :--- |
| **Step 0 (地基)** | `refactor(ai): extract src/ai module and shared redactor/config` | 将 `main.rs` 中的 AI 逻辑下沉至 `src/ai/`（使 `main.rs` 恢复 < 130 行）；提炼共享脱敏截断器 `src/ai/redactor.rs` 与配置解析 `src/ai/config.rs` | `main.rs` < 130 行；脱敏单测全绿 |
| **Step 1 (报告增强)** | `feat(reporter): strongly-typed JSON report with failure snippet and --report-file` | `src/reporter.rs` 引入 `JsonFileReport`；失败项附带脱敏响应片段与断言 `expected`/`actual`；CLI 增加 `--report-file <FILE>` | 报告序列化/反序列化往返单测全绿 |
| **Step 2 (核心领域)** | `feat(ai): attribution and signature-bucket triage in rupost-ai` | 在 `crates/rupost-ai` 实现 `FailureAttributor`、`FailureClassifier`、宽容 JSON/枚举反序列化与桶守恒合并逻辑 | `cargo test -p rupost-ai` 覆盖 정상/乱序/遗漏桶/非法JSON |
| **Step 3 (CLI & Runner 接线)** | `feat(cli): wire --explain (E2) and ai triage / --triage (E5)` | 接线 `rupost test --explain`（含 Top-3 熔断与规则版 B1 基线）与 `rupost ai triage <file>` / `rupost test --triage` | 集成测试覆盖有 AI / 无 Key 降级 / 熔断三路径 |
| **Step 4 (离线闭环)** | `test(e2e): add ai_triage mock dogfooding and verify_features step` | 新增 `examples/ai_triage/mock_ai.json`，更新 `tests/verify_features.sh` 与文档 | 三层测试全绿，0 警告 |
