# E2/E5 统一执行计划 (Master Execution Plan v1.0)

> **创建日期**: 2026-10-28
> **定位**: E2（`test --explain` 断言就地归因）与 E5（`ai triage` 失败聚类）的**唯一执行事实源**——整合 [PRD v1.1](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-28-ai-failure-triage-and-attribution-prd.md)、[实施手册 v1.2（含 §6 Gap Review）](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-28-ai-failure-triage-and-attribution-implementation-plan.md)、代码库陷阱审查表（P0×5 + P1×1）、[dev-plan 技术选型](file:///Users/zsyzzx/project/rust/rupost/.workspace/rupost-ai/doc/plans/2026-10-28-ai-failure-triage-and-attribution-dev-plan.md)。
> **阅读顺序**: 本文档（编排与门禁）→ 实施手册（逐行代码）→ PRD（验收条款原文）。
> **执行者**: Flash 级模型或人；工作区：`rupost-ai` workspace（已位于统一链顶端）。

---

## 1. 目标与范围

**交付两条命令**：
- `rupost test --explain`（别名 `--ai-explain` / env `RUPOST_AI_EXPLAIN=1`）：B1 就地增强——规则版 `[Possible Causes]` 恒打印，AI 可用时追加 `[AI Explanation]`，Top-3 签名去重熔断，**退出码严格由测试决定（失败仍 1）**。
- `rupost ai triage <report.json> [--json|--format]` 与 `rupost test --triage`：B1 批量增强——确定性签名分桶（B 阶段）→ AI 语义归并（C 阶段）→ 桶 ID 守恒回填（D 阶段），无 Key 降级确定性报告 + 黄色警告 + **exit 0**。

**范围外（勿顺手做）**：P0 的 `--last/--response` 与 B4 完整配置链（`~/.rupost/ai.toml`/Ollama 探测）→ 本计划完成后单独收尾；`import curl`（`src/importer/`）；MCP（`src/mcp/`）；`ai gen`。

**预估**: ~6 天（Step 0:1 + 1:1 + 2:1.5 + 3:1.5 + 4:1）。

## 2. 代码现实契约 (Code Reality Contract)

以下 14 条已逐一与仓库真实代码实证核对。**执行中与正文代码冲突时，以本契约为准**；违反任意一条即架构违规。

### 2.1 陷阱修复表（P0/P1，来自陷阱审查 + 实施手册铁律）

| ID | 级 | 真实代码陷阱 | 强制修法 | 落点 |
| :-- | :-- | :-- | :-- | :-- |
| CR-1 | P0 | `AiClient::generate_text(system, user)`、`OpenAiClient::new(api_key, base_url: Option, model: Option)`；错误仅 `AiError::TranslationFailed(String)` 与 `AiError::ParseError(#[from] serde_json::Error)` | 严禁臆造签名；字符串错误只走 TranslationFailed | Step 2 |
| CR-2 | P0 | `tests/ai_assert_test.rs` 契约：输出每行 `# @assert ` 前缀；无 Key exit 1 | `run_ai_assert` 下沉 `src/ai/mod.rs` 时 **100% 原样保留** | Step 0 |
| CR-3 | P0 | `AssertionResult`：`expected: String`（非 Option）、`message: Option<String>`、`stream_event_index: Option<usize>`；构造器 `failure(raw, actual, expected, message)`（actual 第 2 位） | 精确按序使用；report 层 expected 空串折叠为 None | Step 1 |
| CR-4 | P0 | `src/error.rs` **不存在** `RupostError::ConfigError` | 统一用 `RupostError::ParseError(String)` / `Other(String)` | Step 3 |
| CR-5 | P0 | `rupost mock` 顶层必须 `Vec<MockRouteConfig>`（含 `variants`/`response_body`） | `examples/ai_triage/mock_ai.json` 严格对齐 `examples/ai_assert/mock_ai.json` 格式 | Step 4 |
| CR-6 | P1 | 宿主机 `OPENAI_API_KEY` 会污染离线测试 | `build_triage_report_with_client(reports, Option<&dyn AiClient>)` 显式注入；verify 脚本 `env -u` 双变量 | Step 3/4 |

### 2.2 本轮查漏新增事实（Gap Review 实证，v1.2）

| ID | 真实代码事实 | 强制修法 | 落点 |
| :-- | :-- | :-- | :-- |
| CR-7 | 【已证伪】`runner/mod.rs:11` 实际已有 `pub use types::{TestResult, TestSummary};` | 无需任何改动；**禁止重复添加**（E0252） | — |
| CR-8 | `TestSuiteOptions` 现无 Default；CLI 默认 **cookie 启用** | 新增 Default 时 `no_cookies: false`（手册 v1.1 笔误 true 已修正） | Step 1 |
| CR-9 | 旧 `report_json` 字段名与新 `JsonTestReport` 完全一致；`tests/batch_testing_test.rs` 为存量消费者 | 仅允许**新增**字段（expected/actual/response_body_snippet/stream_event_index），零改名 | Step 1 |
| CR-10 | `Response::new(status, headers, body, duration, ttfb, transfer) -> Result`；`Headers` = `reqwest::header::HeaderMap` 别名 | 测试直接用 `HeaderMap::new()` | Step 1 |
| CR-11 | app.rs 渲染块（`:120-146`）与手册 Step 3 挂载代码逐行吻合；`has_failure` 既有 | 挂载点照抄，勿改渲染结构 | Step 3 |
| CR-12 | `DiagnosticsReport` 已派生 Serialize/Deserialize/Clone | `JsonTestReport.diagnose_report` 可直接引用 `crate::http::DiagnosticsReport` | Step 1 |
| CR-13 | `exporter::redact` 三个 pub 函数 + `pub mod redact` 既有 | `src/ai/redactor.rs` 用 `pub use` 复用 + JSON 键掩码 + 512B UTF-8 安全截断；**禁止重复实现** | Step 0 |
| CR-14 | `main.rs` 现 180 行（红线 150） | 下沉后 ≤ 135 行；`wc -l` 断言入门禁 | Step 0 |

## 3. 架构不变量（B0–B4，摘自总体规划）

- **B0**：`src/runner/{executor,batch,engine,...}`、`src/http/`、`src/parser/`、`src/assertion/` 对 `rupost_ai` 零依赖；`rg "rupost_ai" src/runner/ src/http/ src/parser/ src/assertion/` 必须为空。AI 编排（explain/triage）唯一挂载点：`src/runner/app.rs` 渲染阶段允许 `crate::ai`；`redactor` 属纯文本脱敏工具，另被 `reporter.rs` 报告生产路径复用（不涉 AI 编排与判定）。
- **B1**：E2/E5 均为增强型——规则版基线永远可用；AI 缺失/故障/输出非法 → 基线 + 黄色警告，绝不 panic、绝不阻塞判定。
- **B3**：`crates/rupost-ai` 只收纯 DTO（FailureContext/SignatureBucket），零 rupost 内部类型。
- **AI 只在创作/诊断期**：测试成功/失败 100% 由确定性断言决定；AI 绝不改写测试文件。

## 4. 执行序列（Step 0–4 任务卡）

> 每卡 = 任务清单 + TDD 门禁（先红后绿）+ Gap 修正点。代码骨架以实施手册对应章节为准，**叠加本卡 Gap 修正**。每步提交前统一跑：`cargo fmt --check` / `cargo clippy --workspace --all-targets --all-features -- -D warnings` / `cargo test --workspace`，全绿后 `jj describe && jj new`。

### Step 0 — 地基抽取（~1 天）
`refactor(ai): extract src/ai module, config loader and redactor`
1. `run_ai_assert` + `truncate_message` 原样下沉 `src/ai/mod.rs`（CR-2）；`main.rs` ≤ 135 行（CR-14，`wc -l` 入门禁）。
2. `src/ai/config.rs`：`AiRuntimeConfig::from_env()`（空串视同未配置）+ `into_client() -> Option<Arc<dyn AiClient>>`。**env-only**——勿加 ai.toml/Ollama 探测（范围注记 NOTE-1）。
3. `src/ai/redactor.rs`：`pub use` exporter 三函数（CR-13）+ `redact_json_value` 递归掩码 + `truncate_utf8_safe`（is_char_boundary，禁裸切片）+ `redact_and_truncate_body`（512B）。
4. **GAP-1【已证伪】**：`runner/mod.rs:11` 已有 `pub use types::{TestResult, TestSummary};`——**禁止重复添加**（会触发 E0252），Step 3 的 `use crate::runner::TestResult` 直接可用。
- 门禁测试：redactor 矩阵（JSON 掩码/中文截断边界/幂等）×10、config 空串回退 ×4、`ai_assert_test` 存量 4 测试全绿、`wc -l main.rs` < 150。

### Step 1 — 强类型报告与 --report-file（~1 天）
`feat(reporter): typed JsonFileReport with failure snippet and --report-file`
1. `JsonFileReport/JsonTestReport/JsonAssertionReport`（serde 全兼容 CR-9；`#[serde(default)]` 前向兼容；失败项 `response_body_snippet` 经 redactor 脱敏 512B；URL 经 `redact_url`）。
2. **GAP-6/CR-3**：`JsonAssertionReport` 含 `stream_event_index: Option<usize>`（`#[serde(default, skip_serializing_if)]`）。
3. **GAP-8/CR-8**：`TestSuiteOptions` 加 `report_file/explain/triage` 字段 + Default（`no_cookies: false`）。
4. `--report-file <FILE>`：app.rs 报告块后落盘（`File::create` 即可，不建目录——路径错误报 IoError）。
- 门禁测试：序列化↔反序列化往返（含脱敏断言与 expected/actual 字段）；旧报告（无新字段）可读入；`batch_testing_test.rs` 存量全绿（CR-9）。

### Step 2 — crate 纯算法层（~1.5 天）
`feat(ai): attribution and signature-bucket triage in rupost-ai`
1. `json_util.rs`：`extract_json_block`（剥 ```json 围栏 + 首尾噪音；**严禁 UTF-8 裸切片**——铁律 #2）。
2. `attribution.rs`：`FailureContext/FailedAssertionInfo/AttributionAdvice` + `FailureAttributor`。**GAP-9**：`fix_suggestion` 加 `#[serde(default)]`；`root_cause` 空值检查（既有）。system prompt 固化 schema `{root_cause, fix_suggestion, suggested_assert}`，serde alias 兼容 `summary/suggested_action`。
3. `triage.rs`：`SignatureBucket`（**GAP-10**：三个 skip 字段补 `#[serde(default)]`）、`FailureCategory`（from_loose_str 别名表 + `Other(String)` 无损兜底 + 手写 Serialize/Deserialize）、`FailureClassifier`（**桶 ID 守恒**：consumed_ids 追踪 + 漏桶自动补确定性簇 + 计数恒等）、`TriageReport`、`bucket_to_deterministic_cluster`。
- 门禁测试：围栏/噪音提取 ×3；正常/乱序/漏桶/桶 ID 幻觉/非法 JSON/未知分类 ×6（mockall）；**计数守恒断言**（Σcluster.count == Σbucket.count）。

### Step 3 — 规则基线 + 控制器 + CLI 接线（~1.5 天）
`feat(cli): wire --explain (E2) and ai triage / --triage (E5)`
1. `src/ai/rules.rs`：`infer_possible_causes`（6 类规则表：鉴权/4xx 契约/5xx/超时连接/exists 断言/变量未解析 warning）；`normalize_url_pattern`（数字/UUID → `:id`，前 4 段）；`build_signature_buckets`（BTreeMap 聚合 + 降序 + MAX 20 桶尾部合并）；`context_from_json_test_report`；**GAP-2**：新增 `failure_signature(ctx) -> String`（`路径模式|status|错误类型`）。
2. `src/ai/explain.rs`：`ExplainSession`——规则版恒打印；**GAP-2**：`explained_signatures: HashSet` 签名去重（同签名直接跳过 AI，规则版仍打印）；`MAX_AI_EXPLAIN_CALLS = 3`（按去重后不同签名计）+ `CIRCUIT_BREAKER_THRESHOLD = 2` 连续错误熔断 + limit_warned 单次提示（提示语指向 `--triage`）。
3. `src/ai/triage.rs`：`build_triage_report_with_client`（显式注入，CR-6）/`build_triage_report`（env 路径，**GAP-3**：client 为 None 时 `eprintln` 黄色 `[!] AI 不可用（未检测到配置），已输出确定性签名分组结果`）；`format_triage_markdown`；**GAP-11**：`run_inline_triage` 全绿时也打印单行 "All N requests passed"。
4. CLI 接线（CR-4/CR-6/CR-11）：`test --explain/--ai-explain/--triage` + env 合并；`AiCommands::Triage { report_file, json, format }`；app.rs 渲染块挂载（`--report json` 模式保持纯 JSON 不掺杂）。
- 门禁测试：rules 决策表 ×8；explain 单测（去重配额/熔断/limit 提示）×4；triage 控制器（有 AI/无 Key 降级/守恒）×3；`--json` 输出。

### Step 4 — 离线闭环与归档（~1 天）
`test(e2e): ai_triage/explain dogfooding and verify_features smoke`
1. `examples/ai_triage/mock_ai.json`：两条路由——`POST /v1/chat/completions`（固定聚类建议，格式对齐 CR-5）+ `GET /v1/users/:id` → 401（**GAP-5**：供真实失败）。
2. `examples/ai_triage/failing_suite.http`：`GET {{base_url}}/v1/users/1` + `@assert status == 200`。
3. `tests/ai_triage_test.rs`：① 确定性分桶（注入 None，免疫宿主机 env，CR-6）×2；② wiremock AI 增强聚类 ×1；③ **GAP-4**：E2 无 Key——401 mock + `test --explain`（env -u）→ 含 `[Possible Causes]`、不含 `[AI Explanation]`、exit 1；④ **GAP-5 dogfooding 闭环**：起 mock(9002) → `test failing_suite.http --var base_url=... --report-file report.json`（exit 1）→ `ai triage report.json --json` → 含 `"category": "Auth"`。
4. verify_features.sh：中文横幅前追加确定性 triage 冒烟 + dogfooding 步骤（`env -u` 双变量、端口 9002、`set -e` 下 `|| VAR=$?` 捕获）。
5. 归档：`progress_summary.md`、`checkpoint.md`、**GAP-12**：`directory_structure.md`（新模块条目）+ `CHANGELOG.md`（Added 两行）。
- 门禁测试：`bash tests/verify_features.sh` 全量 PASS（零外网）。

## 5. 追溯矩阵

| 验收 | 覆盖 |
| :-- | :-- |
| PRD US1（就地归因 + 熔断 + 降级 exit 1） | Step 3 explain + GAP-2/4 + Step 4-③ |
| PRD US2（聚类 + 计数守恒 + `--json` + 降级 exit 0 + 黄色警告） | Step 2 classifier + Step 3 triage + GAP-3 + Step 4-①②④ |
| 陷阱 CR-1…CR-6 | Step 0/2/3/4 各卡内联 |
| Gap GAP-1(证伪)/GAP-2…GAP-12 | Step 0(0)/1(2)/2(2)/3(4)/4(3) 内联 |
| 成功指标 | DoD 总清单（§7） |

## 6. 技术选型摘要（细目见 dev-plan §4）

零新增第三方依赖（T1–T10 复核维持）；关键裁决：prompt 固化 + 宽容解析（非 response_format）；**E2 串行调用**（T3 有界并行 superseded——输出交错需顺序，熔断+配额已限最坏延迟）；探测超时/字符预算/手绘终端输出/serde-default 前向兼容维持原选型。

## 7. DoD 总清单

- [ ] `wc -l src/main.rs` < 150（预期 ~130）
- [ ] `rg "rupost_ai" src/runner/ src/http/ src/parser/ src/assertion/` 为空（app.rs 的 `crate::ai` 不算）
- [x] GAP-1【已证伪】：`runner/mod.rs` re-export 已存在，勿重复添加
- [ ] `cargo test --test ai_assert_test` 存量 4 测试全绿（CR-2）
- [ ] fmt / clippy(-D warnings) / `cargo test --workspace`（≥ 571，0 失败；原估 590 按实测修订）/ `bash tests/verify_features.sh` 全 PASS
- [ ] 计数守恒：Σcluster.count == failed_requests（集成断言）
- [ ] 同签名失败只调一次 AI（GAP-2 mock 计数断言）
- [ ] 无 Key：`ai triage` stderr 含 "AI 不可用" 且 exit 0；`test --explain` 含规则基线且 exit 1（GAP-3/4）
- [ ] dogfooding 闭环：`--report-file` → `ai triage --json` 含目标分类（GAP-5）
- [ ] 全输出 `grep` 无敏感值（脱敏矩阵 + 512B 截断）
- [ ] 归档四件套 + directory_structure + CHANGELOG（GAP-12）

## 8. 风险与协调

| 风险 | 缓解 |
| :-- | :-- |
| LLM 聚类文案质量差 | 注记式守恒回填（LLM 只贴标签不移成员），结构永远正确；规则版文案兜底 |
| `--explain` 拖慢失败反馈 | Top-3 配额 + 连续 2 错熔断 + 串行但先打印规则版（体验渐进） |
| 并行会话冲突 | 执行期间与其他会话约定链上写权限；每次 `jj new` 前先 `jj log` 确认链顶 |
| 强类型报告破坏存量消费者 | CR-9 零改名 + workspace 测试门禁 |
| 重构 Step 0 引入回归 | 纯搬移 + `ai_assert_test` 契约锁 + 行数断言 |

**执行工作流**：每步 `jj describe && jj new`；本计划与实施手册/PRD 已在链上，执行者按 Step 顺序推进；发现手册与真实代码冲突时以 §2 契约为准并回写手册。
