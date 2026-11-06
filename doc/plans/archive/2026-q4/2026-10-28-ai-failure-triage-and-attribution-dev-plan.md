# E2/E5 整体开发计划与技术选型 (Stage 1 Execution Plan)

> **创建日期**: 2026-10-28
> **被实施对象**: [E2/E5 PRD v1.1](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-28-ai-failure-triage-and-attribution-prd.md)（`rupost test --explain` 断言就地归因 + `rupost ai triage` CI 失败聚类）
> **性质**: 开发执行计划——PRD 已回答"做什么/为什么"，本文回答"怎么排/用什么/怎么验"。父纲：[MVP 整合计划](file:///Users/zsyzzx/project/rust/rupost/.workspace/rupost-ai/doc/plans/2026-10-27-ai-mvp-consolidation-plan.md)、[总体规划 B0–B4](file:///Users/zsyzzx/project/rust/rupost/.workspace/rupost-ai/doc/plans/2026-10-27-ai-master-roadmap.md)
> **代码现状核实**: PRD 的 P-1~P-5 全部复核属实（main.rs 180 行 / `exporter/redact.rs` 131 行 / `report_json` 手写 `json!` 丢 expected-actual / `--report` 不支持文件路径 / `TestResult.status` 为 `Option`）

---

## 1. 范围收拢：本 PRD 与 MVP 计划的对齐

| MVP 计划条目 | 本 PRD 覆盖情况 |
| :-- | :-- |
| R1 (P0) 零配置链 + redactor | **Step 0 全覆盖**（`src/ai/{config,redactor}` + B4 探测链） |
| R1 (P0) `--last/--response` 上下文断言 | **不在本 PRD**——基建就绪后作为收尾小项（~1 天），紧随 Step 4 |
| R7 (E2) 失败归因 | Step 1–4 全覆盖（含熔断与 B1 基线） |
| R8 (E5) triage | Step 1–4 全覆盖（含确定性签名分组与计数守恒） |
| R4 `import curl` / R9 MCP | **不在范围**——`src/importer/` 与 `src/mcp/` 独立模块，按模块归宿约定另行立项（见 MVP 计划 §3 与归宿映射讨论） |

**结论**：本 PRD 实质是 MVP 的 **M1 核心提前合体**（R1 地基 + R7）+ **M2 的 R8**，一次交付后 M1/M2 仅剩 `import curl` 与 P0 收尾。执行顺序因此调整为：本计划（~6 天）→ P0 收尾 `--last`（1 天）→ R4 import curl（3 天）→ M3 MCP。

**工作区约定**：实施在 `rupost-ai` workspace 进行；PRD 文件目前在 default workspace 工作副本未提交，需在 default 侧提交一次（或随实施分支合并），避免双头引用漂移。

## 2. 模块归宿与接线约束（实施硬约束）

| 代码 | 归宿 | 约束 |
| :-- | :-- | :-- |
| `src/ai/{mod,config,redactor,explain,triage}.rs` | 主包 AI 编排层 | 唯一允许同时 import rupost 内部类型与 `rupost_ai` crate 的层 |
| `crates/rupost-ai/src/{attribution,triage}.rs` | 独立 crate | 只收纯 DTO（`FailureContext`/`SignatureBucket`），**零 rupost 类型**（B3） |
| `src/reporter.rs` 强类型报告 | 表现层 | `JsonFileReport` 全字段 `#[serde(default)]` 前向兼容（旧报告可读入） |
| `src/runner/` | **零改动**（除透传既有 `TestResult`） | B0：`--explain`/`--triage` 均在 `run_test_suite` **返回后**由 adapter 层（main → `src/ai`）后处理；引擎对 `rupost-ai` 依赖图上不可达 |
| 脱敏 | `src/ai/redactor.rs` 统一入口 | 内部复用 `exporter::redact` 的 Header/URL 规则 + 新增 JSON 敏感键掩码与 1KB 截断；**不复制粘贴 exporter 逻辑** |

## 3. 任务拆解与提交序列（PRD Step 0–4 展开）

每步均含 TDD 门禁：先落测试（红）→ 实现（绿）→ 提交前三步（fmt / clippy -D warnings / `cargo test --workspace`）。

| Step | 提交 | 任务卡 | 测试门禁（先红后绿） | 预估 |
| :-- | :-- | :-- | :-- | :-- |
| **0 地基** | `refactor(ai): extract src/ai module and shared redactor/config` | ① `run_ai_assert` 从 main.rs 下沉 `src/ai/mod.rs`（main.rs → <130 行）；② `src/ai/config.rs`：B4 链 env > `~/.rupost/ai.toml`（含 api_key 字段拒绝）> Ollama 探测（**探测用独立 2s 超时请求**，不复用 30s 客户端）；③ `src/ai/redactor.rs`：包 exporter::redact + JSON 键掩码 + 截断 | config 优先级矩阵 11 例（沿用 P0 测试方案 TC-C01–C11）；redactor 矩阵 10 例（TC-R01–R10）；main.rs 行数断言（<130，防再腐化） | 1 天 |
| **1 报告** | `feat(reporter): strongly-typed JSON report with failure snippet and --report-file` | ① `JsonFileReport/JsonTestReport/JsonAssertionReport` 强类型化（含 `expected/actual/response_body_snippet`，失败时经 redactor 脱敏 1KB）；② CLI `--report-file <FILE>`；③ 保持旧字段名不变（序列化兼容） | 序列化↔反序列化往返；旧报告（无新字段）可读入；失败项 snippet 脱敏断言 | 1 天 |
| **2 领域** | `feat(ai): attribution and signature-bucket triage in rupost-ai` | ① `attribution.rs`：`FailureAttributor`（prompt 固化 → `AttributionAdvice` 宽容解析）；② `triage.rs`：`FailureClassifier`（M 个签名桶 → LLM 合并建议 → **桶 ID 守恒回填**：AI 返回的 test_ids 必须映射回真实桶，未知 ID 丢弃并告警，总数恒等）；③ `FailureCategory` 未知值落 `Other(String)` | crate 单测：正常/围栏包裹/非法 JSON/未知分类/桶遗漏/桶 ID 幻觉（mockall） | 1.5 天 |
| **3 接线** | `feat(cli): wire --explain (E2) and ai triage / --triage (E5)` | ① `test --explain`（别名 `--ai-explain`、env `RUPOST_AI_EXPLAIN=1`）：规则版基线 `rule_based_causes` → AI 追加 → **Top-3 签名去重熔断**；② `ai triage <file>` 与 `test --triage`（内存路径复用同一管线，输入从 `Vec<(PathBuf, Vec<TestResult>)>` 而非文件）；③ 签名归一化：`HTTP {status} @ {前两段路径}/*`、`ERROR({首行错误类}) @ …`、`HTTP 200 \| assert: {归一化断言}`；④ exit code 语义（E2 失败仍 1；E5 恒 0） | 集成三路径：有 AI / 无 Key 降级（黄 `[!]`）/ 熔断触发；计数守恒（42=31+9+2 型断言）；`--json` 输出 | 1.5 天 |
| **4 闭环** | `test(e2e): ai_triage mock dogfooding and verify_features step` | `examples/ai_triage/mock_ai.json`（chat completions 固定聚类建议）+ verify_features.sh 步骤 9 + examples README + CHANGELOG/progress_summary 归档 | L3 脚本 PASS；全量 ≥ 590 测试 0 失败 | 1 天 |
| **收尾** | P0 `--last/--response`（另一 PRD 剩余） | 复用 Step 0 的 config/redactor/context 通道 | P0 测试方案既有用例 | 1 天 |

**并行机会**：Step 0 与 Step 2 无相互依赖（crate 领域层可先行）；R4 import curl（`src/importer/`，零 AI）可完全并行开发。

## 4. 技术选型（决策表）

| # | 决策点 | 候选 | **选择** | 理由 |
| :-- | :-- | :-- | :-- | :-- |
| T1 | LLM 接入 | `async-openai` / 裸 reqwest `AiClient` | **既有 `AiClient` trait + `OpenAiClient`** | 已交付已测试；B3 边界干净；引 SDK 收益为零（只用 chat completion 一个端点） |
| T2 | 结构化输出保证 | `response_format: json_object`（OpenAI 专有）/ JSON Schema 强约束 | **prompt 固化 + 宽容解析**（剥围栏 → serde 宽容 → `Other(String)` 兜底） | Ollama/DeepSeek 等兼容端点对 response_format 支持参差；宽容解析已在 translator 验证；保底路径是丢弃+警告而非失败 |
| T3 | LLM 调用并发 | 串行 / `futures-util::stream::buffer_unordered(3)` | **有界并行（3）** | 签名桶 ≤15、熔断 Top-3，总量小但串行最坏 ~18×秒级；futures-util **已在依赖树**，零新增 |
| T4 | 脱敏实现 | 新写 / 复用扩展 | **扩展 `exporter::redact` + 新增 JSON 键掩码层**，统一入口 `src/ai/redactor.rs` | DRY（P-2 裁决）；exporter 侧规则已过测试 |
| T5 | 配置解析 | 手写 / `toml` crate + serde | **toml（已在树）** | `~/.rupost/ai.toml` 强类型；api_key 字段拒绝逻辑在反序列化校验层 |
| T6 | Ollama 探测超时 | 复用 30s 客户端 / 独立短超时 | **独立 `GET {base}/v1/models`，2s per-request 超时** | 探测失败必须快（否则零配置反成 30s 卡顿）；reqwest per-request timeout 原生支持 |
| T7 | Token 预算 | `tiktoken-rs` 精算 / 字符上限启发式 | **字符上限**（上下文 1KB、桶 M≤15、总发送 O(M)） | 不为此引重依赖；PRD 的复杂度上限设计已使精确计数失去意义 |
| T8 | triage 终端输出 | `comfy-table` / 手绘 colored | **手绘**（匹配 reporter 既有 HSL 美学），`--json` 机器可读另路输出 | 聚类简报是叙事型排版（标题/样本/根因），表格库反而受限；comfy-table 留给纯表格场景 |
| T9 | 报告 schema 演进 | 版本号字段 / 全字段 `#[serde(default)]` | **serde default + 旧字段名不动** | 前向兼容免版本协商；`--report-file` 产物可被旧版读不炸 |
| T10 | 签名归一化 | 完整 URL / 路径模式 | **前两段路径 + `*` 通配**，数字段折叠 | PRD 样例（`/api/v1/*`）实证；实现 ~10 行纯函数，决策表单测锁定 |

**总选型结论：零新增第三方依赖**（T1–T10 全部复用既有依赖树或纯标准库）。唯一新增是 crate 两个源文件与主包一个模块——与"极致简洁"原则一致，也消除供应链审查成本。

## 5. 关键设计决策（PRD 未尽事项的补裁决）

1. **熔断去重键** = 归一化签名（与 triage 同一函数），保证 E2 的 Top-3 与 E5 的桶语义一致，用户从 `--explain` 迁移到 `--triage` 看到同一种归类语言。
2. **`--triage` 与 `ai triage <file>` 单管线双入口**：前者内存消费 `Vec<(PathBuf, Vec<TestResult>)>`，后者读 `JsonFileReport`——素材提取层抽象为 `trait FailureSource`，两条路径共享 B/C/D 阶段。
3. **AI 合并建议的守恒回填算法**：以 Rust 侧桶为事实源；LLM 只能输出"桶 ID → 标题/类别/根因"的**注记**，不允许移动成员——从机制上消灭计数不守恒与幻觉 ID（比"校验后丢弃"更强的设计）。
4. **规则版基线的规则来源**：`rule_based_causes` 首版覆盖 6 类（401/403 鉴权、4xx 契约、5xx 服务端、超时/连接错误、断言 exists 失败、变量未解析 warning），每类一条固定文案 + 一条动作建议——规则表驱动，单测锁定。
5. **`RUPOST_AI_EXPLAIN=1` env 与 flag 的优先级**：flag 显式 false 不存在（开关型），env 仅在 flag 未传时生效（clap `default_value_t` + env fallback）。

## 6. 风险登记

| 风险 | 概率 | 缓解 |
| :-- | :-- | :-- |
| LLM 聚类建议质量差（桶标题/根因文不对题） | 中 | 注记式回填（§5.3）保证结构永远正确；质量只影响文案层；规则版文案兜底 |
| `--explain` 拖慢失败反馈（3 次串行等待） | 中 | T3 有界并行 + 熔断上限；失败先打印规则版，AI 追加打印（流式体验） |
| 强类型报告改动破坏既有 CI 消费者 | 低 | 旧字段名零变更 + serde default；既有 JSON 报告测试全量回归 |
| 双 workspace 引用漂移（PRD 在 default 未提交） | 中 | §1 约定：实施前在 default 侧提交 PRD 一次 |
| Step 0 重构引入回归 | 低 | 纯下沉搬移 + 行数断言 + 全量测试门禁 |

## 7. 时间线与成功标准

**总预估 ~6 天**（Step 0:1 + 1:1 + 2:1.5 + 3:1.5 + 4:1），加 P0 收尾 1 天。成功标准继承 PRD US1/US2 全部 Then/And 条款，机器可验指标：① 计数守恒（42=Σ簇）；② 熔断日志行出现；③ 无 Key 降级路径 exit code 语义正确（E2=1、E5=0）；④ `grep` 全输出无敏感值；⑤ verify_features.sh 步骤 9 零外网 PASS；⑥ 全量测试 ≥ 590。
