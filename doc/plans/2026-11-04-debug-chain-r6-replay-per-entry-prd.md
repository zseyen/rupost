# RuPost 调试链收尾 PRD：R6 replay per-entry 结构化结果 + T5 export 搭车 + AI 预留桥

> 由 `make prd` 模板规范手工生成；产物落 `doc/plans/`（随 rupost-ai 执行线交付）。

- **状态**：**Done（2026-11-04 交付）**；生命周期 Proposed → Accepted → In Progress → Done
- **日期**：2026-11-04
- **执行队列**：并入主执行文档（`doc/plans/2026-10-27-master-execution-runbook.md`）T3 卡 + T5 卡搭车裁决
- **范围基线**：**rupost-ai 工作区执行线**（工作副本 @ d915378c，main 尖 51784fe1 在其祖先链——2026-11-03 已 rebase，见 `checkpoint.md:266`）。该线已交付 E2（`--explain`）、E5（`ai triage`）、P0（`--last`）、R7（`--on-failure` 钩子 + trace 锚点）——调试链 D3/D4/D5 在此线上闭环，主线视角的"未合并"状态不属本 PRD 范围。
- **关联文档**：[DX-Debug PRD](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-28-developer-debug-experience-prd.md)（R6 定义）、[主执行 runbook](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-27-master-execution-runbook.md)（T3/T5 卡）、[快照路线图](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-07-01-snapshot-replay-roadmap.md)（T5 搭车承诺）、[E2/E5 PRD](file:///Users/zsyzzx/project/rust/rupost/.workspace/rupost-ai/doc/plans/2026-10-28-ai-failure-triage-and-attribution-prd.md)（FailureContext 契约）、[需求总矩阵](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-28-requirements-matrix.md)（四格纪律）
- **事实源分工**：本文件只承载需求与规格，**不记进度**——进度唯一事实源是 `doc/plans/progress_summary.md`，完成后登记批次行。
- **兜底分级声明（准入强制，per 路线图 §6.1）**：R6 = **确定性需求（B0 领域）**——零 AI 参与的执行侧增强；AI 预留桥仅是数据形状约定（v1 建桥不接 CLI），AI 归因消费属 E2/E5 后续扩展。

---

## 一、背景与痛点（全部实证）

### 1.1 调试链现状（工作区执行线视角，2026-11-04 双子代理审查核实）

| 链环 | 状态 | 证据 |
| :-- | :--- | :--- |
| D0 触发 / D1 观察 / D2 初诊 | ✅ | DBG-1/DBG-2/R3 主体（`tests/report_schema_test.rs:143-205` 13 键锁定）均闭环 |
| D3 归因（R4 `--explain`） | ✅ 本线已交付 | `src/ai/explain.rs` + `src/ai/rules.rs` |
| D4 分诊（R5 `--triage`） | ✅ 本线已交付 | `src/ai/triage.rs:83` + `tests/ai_triage_test.rs` |
| D5 取证（R7 钩子+trace） | ✅ 本线已交付 | `src/runner/on_failure.rs` + `tests/on_failure_test.rs`，ledger 73c1d9b6 "D5 closed" |
| **D4/D6 交界：replay 定位到条（R6/T3）** | 🕳 **本 PRD** | `replayer.rs:16-20` 仅 total/passed/failed 三计数 |
| D7 沉淀 | ✅ | generate 语义化 + 凭据模板化 |

### 1.2 痛点实证

- **P-1 重放失败不可定位**：`rupost replay` 跑完，程序内部只记得 `failed: N` 计数（`src/runner/engine/replayer.rs:16-20`）——"哪 N 条失败"随终端滚动丢失，无结构可供脚本/AI 消费。runbook T3 卡定义此为"取证承重墙"（T4 Smart Diff 的前置；与 R5 强类型报告按 DX PRD 原文口径为"同批共享结构"关系，R5 本线已交付）。
- **P-2 重放结果对 CI 无效**：CLI dispatch 丢弃 `ReplayReport` 返回值（`src/cli.rs:770-771`），**全失败仍 exit 0**——脚本/CI 无法用退出码判断重放成败。
- **P-3 快照供给链断裂**：`rupost history export` 是占位桩（`src/cli.rs:738-740` 只打印 "not implemented"）。快照路线图（`:35`）与需求矩阵 T5 行（`:87`，"T5 随 T3/T4 搭车"）承诺搭车——矩阵 R6 行（`:86`，"R6 随 T5 搭车"）与 `:87` 搭车方向记载互相矛盾，属台账张力，随 Step 4 勘误登记；无 export 则用户无快照文件可重放（只能靠测试现场 `--save-snapshot`）。
- **P-4 台账漂移**：需求矩阵 R3 注记（`:83`，"diagnose 命令自身的 json 契约仍待"）写于 2026-10-28，早于主线 e5a957c0 落地已过期；`checkpoint.md:254` "R1–R10 中最后一个未实现的确定性需求落地"与 `:266` "仅余 R3/R6" 自相矛盾。

---

## 二、使用场景与用户画像

### 2.1 画像定位（四格纪律，per 需求总矩阵 §6.1）

| 格 | 值 | 说明 |
| :-- | :-- | :--- |
| **环境** | 开○ / **测●** / 产○ | CI 靶场是主消费环境（重放验证 + 退出码门禁）；生产为灰度比对辅助场景 |
| **阶段** | M2 / S6 | 与 R5 强类型报告同批认知层；CI 批量旅程 |
| **画像** | **小赵（QA/回归分诊）主画像**；小李（后端/prod 灰度比对）次画像 | 小张（前端联调）经 replay 契约比对间接受益 |
| **流式标签** | **不适用** | replay 仅走 HTTP 执行器；快照模型（`RequestSnapshot`/`ResponseSnapshot`）不含 SSE/WS 帧——边界本身写入 §5 非目标 |

**安全注记**：①快照原样存凭据头与响应体（`RequestSnapshot.headers`/`ResponseSnapshot.body`），replay 外发面 = 录制凭据打向 `--target` 灰度或原始 host；②本需求是 X5（写方法/replay → 告警+显式确认）的直接客体，注记引用矩阵 `:48`；③R6 本体 B0 零 AI；桥接输出若未来进 AI 归因受 X6 出站默认关约束；④prod 取证动作审计留痕关联矩阵 `:35`。

**三问判定记录**：①共性——replay 定位到条是所有快照消费者的共同缺口（T4/R5/MCP 潜在消费方），非单场景私有 ✓；②增益——在"终端 API 回归引擎"定位下，重放结果的机器可读是快照 MVP 的收尾必经步，非堆功能 ✓；③定位——属调试链 D4/D6 交界确定性欠账，与 AI 路线（B1 增强型）无重叠，AI 侧仅按用户要求预留数据形状接口 ✓。

### 2.2 使用场景（GWT）

**SC-A 重放定位到条（小赵，主场景）**
> Given 一份含 3 条请求的快照（2 成 1 败），When `rupost replay snap.json`，Then 终端汇总块输出 `Total: 3, Passed: 2, Failed: 1` 且随后列出失败条目（`#2 POST http://target/login (expected 200, got 401)` 形态）；结构层 `ReplayReport.entries` 可编程分辨具体条目；有失败时进程 exit 1。

**SC-CI 重放门禁（小赵）**
> Given CI 脚本以 `rupost replay snap.json` 作为回归门禁，When 任一条目失败，Then 进程退出码非零，脚本可感知中断。

**SC-历史导出（小赵/小李）**
> Given 历史库中有条目（含早期无响应体的 `body=None` 旧条目），When `rupost history export`，Then 导出快照套件文件；`body=None` 条目**占位导出**——复用既有 `From<HistoryEntry> for SnapshotEntry`（`src/history/model.rs:88-103`），body 置既有占位标记文本 `Old snapshot: body not recorded`，条目保留可追溯；导出文件可直接 `rupost replay` 成功加载。

**SC-AI 预留（设计级场景，非用户级）**
> Given 一条 mismatch 的 `ReplayEntryResult`，When 调用 `context_from_replay_entry`，Then 产出脱敏后的 `FailureContext`，可直接喂入既有 `infer_possible_causes` / `failure_signature` / `build_signature_buckets` 三台归因机器并获得有意义输出（测试级端到端守护；v1 无 CLI 暴露）。

---

## 三、需求清单

| # | 需求 | 优先级 | 验收标准 | 确定性 |
| :-- | :--- | :-- | :--- | :-- |
| **R6-a** | `ReplayEntryResult` 结构体（字段见 §4.1），`ReplayReport` 增 `entries: Vec<ReplayEntryResult>`；`replay_entry` 由 `Result<bool>` 改为返回条目（传输层失败——连接/超时/URL 解析 `replayer.rs:114`/header 还原 `:118`——均落条目，不再 Err 分叉） | P1 | 混合成败（3 条 2 成 1 败，与 SC-A 同规模）单次运行：`entries` 逐条分辨 + count 守恒（`passed+failed=total=entries.len()`，error 条目归 failed，**二分不三桶**）；serde 往返测试 | ✅ 确定性 |
| **R6-b** | 终端失败条目清单块：汇总块后列失败条目；**0 失败不打印该块**；0 条目快照 `entries=[]` 且聚合全 0（既有行为保持） | P1 | stdout 断言测试（含块内容与空态两形态） | ✅ 确定性 |
| **R6-c** | CLI 退出码：`failed > 0` → exit 1（快照损坏的 Err 路径保持既有非零行为） | P1 | 集成测试断言退出码双向（全成 exit 0 / 有败 exit 1） | ✅ 确定性 |
| **R6-d** | AI 预留桥 `context_from_replay_entry(&ReplayEntryResult) -> FailureContext`（映射与脱敏见 §4.3），`mod tests` 表驱动单测守护（对齐 `context_from_json_test_report` 先例），**v1 不接 CLI**；PRD 写明消费落点（E2 报告摄入/归因路径）与解冻条件（fast-follow 立项） | P2 | 桥映射决策表（三形态 × 脱敏）+ 桥产物喂 `infer_possible_causes`/`failure_signature` 端到端单测 | ✅ 确定性（数据形状预留） |
| **T5** | `history export` 桩转实：复用 `write_snapshot_suite` + `SelectionStrategy::Last`（CLI 面已存在 `--last N`/`-o --output`，零新增参数，导出规格见 §4.6）；`body=None` 旧条目占位导出（复用既有 `From` 转换，D9） | P1 | 桩锁定测试先行失败驱动替换（`RUPOST_HISTORY_DIR` tempdir 隔离）；导出文件经 `replay` 加载成功；`body=None` 条目保留且 body 为占位标记文本；`--last` 语义 = 最近 N 条**条目**（D10） | ✅ 确定性 |
| **R3-壳** | `run_diagnose` CLI 壳层（`src/http/mod.rs:28-40` json 分支 println + 错误 exit 路径）补测试 | P2 | 壳层行为测试（可顺带，不阻塞 R6） | ✅ 确定性 |
| **台账** | 勘误矩阵 R3 注记与 `:86`/`:87` 搭车方向矛盾、DX PRD R3/R7 状态列、`checkpoint.md:254/266` 矛盾措辞、runbook T3 卡"快照引用"字段口径注记（§4.1 自包含设计取代） | P2 | 随交付原子提交（矩阵 §6.3 "登记张力、不擅自归位"纪律） | — |

---

## 四、数据模型与设计决策

### 4.1 `ReplayEntryResult`（落 `src/runner/engine/replayer.rs`，`src/runner/mod.rs:16` 加 re-export）

**复用原则**：字段形状全部对齐既有约定，不发明新类型；8 字段平铺无嵌套。

| 字段 | 类型 | 复用对齐来源 |
| :-- | :-- | :--- |
| `request_number` | `usize`（**1-based**） | 对齐终端 `#N` 打印（`replayer.rs:52-55`）与 `JsonTestReport.request_number` |
| `method` / `url` | `String` | 从 `entry.request` 复制，结构体自包含（JSON/AI 桥消费者免持 suite）；`url` = **`--target` 覆写后实际请求 URL**（覆写 `.ok()` 静默失败时记录真实发出的 URL） |
| `expected_status` | `u16` | 同 `ResponseSnapshot.status`（非 Option，旧格式缺字段在套件解析期整体失败，既有行为） |
| `actual_status` | `Option<u16>` | 同 `FailureContext.status_code` / `TestResult.status` 约定：传输/解析失败 = `None` |
| `matched` | `bool` | 既有 `status_matched` 语义；**状态码精确相等保持不变**（等价类/body diff 归 T4） |
| `duration_ms` | `u64` | 对齐 `JsonTestReport.duration_ms` / `HistoryEntry.duration_ms`（`as_millis() as u64` 先例 `reporter.rs:346`）；**不用 `Duration`**（`{secs,nanos}` 序列化仅诊断细分计时先例，避免 JSON 噪音） |
| `error` | `Option<String>` | 捕获该条目**传输层**失败（网络/超时/URL 解析失败 `replayer.rs:114`/header 还原失败 `:118`，文本取首行）；mismatch 不置 error——三形态语义见 §4.2 |

serde：`#[derive(Debug, Clone, Serialize, Deserialize)]`；Option 字段 `#[serde(default)]` 起步，若需省略输出则 `default`+`skip_serializing_if` **成对**（对齐 `ResponseMeta.body` / `FailureContext.trace_id` 先例；`JsonTestReport` 本身无 skip）。

### 4.2 三形态映射表（error 语义定案）

| 形态 | `actual_status` | `matched` | `error` | 聚合归类 |
| :-- | :-- | :-- | :-- | :-- |
| match | `Some(expected)` | `true` | `None` | passed |
| mismatch | `Some(actual)` | `false` | `None`（expected/actual 差值由结构体自身字段承载，error 保持传输层语义） | failed |
| 条目级失败（网络/超时/URL 解析/header 还原） | `None` | `false` | `Some(原因)` | failed |

**补充定案**：error 形态的 `duration_ms` = 请求发起至失败的**实际耗时**——`elapsed` 计算移入执行 match 两臂（现状 `?` 在 `elapsed` 计算之前传播，`replayer.rs:129-131`），禁止硬编码 0；header 非 UTF-8 值静默跳过维持现状（`replayer.rs:117`，非目标 6 容错语义）；"execute 返回 Ok 但 body 读取失败"分支不存在（`client.execute` 返回的 body 已是完成读取的 String，读取失败自然冒泡落 error 形态）。

### 4.3 AI 预留桥（`src/ai/rules.rs`，与 `context_from_json_test_report` 同构对称）

| FailureContext 字段 | 映射 | 约束 |
| :-- | :-- | :--- |
| `request_name` | `None` | 不合成 "replay#n"（无消费方，bucket label 已有 None 降级分支 `rules.rs:273-282`） |
| `method` / `url` | 直取 | **`url` 必经 `redact::redact_url`**（快照 URL 可能带 query token） |
| `status_code` | `actual_status` | — |
| `error` | `error.map(redact_free_text)` | **防御性脱敏，Gap Review P3 安全不变量**（先例 `rules.rs:27-32`、`:280-282`） |
| `failed_assertions` | `vec![]` | replay 无断言 |
| `response_body_snippet` | `None` | v1 不采集（T4 Smart Diff 扩展面） |
| `warnings` / `trace_id` | `vec![]` / `None` | replay 不采集响应头 trace（R7 管线不覆盖 replay 是本 PRD 边界） |

**守护方式**：`mod tests` 表驱动单测（AGENTS.md Stage 2&3 纪律；先例 `rules.rs:367`）。**已知限制（记录一行）**：mismatch 条目 error=None 且 failed_assertions 空时，`failure_signature` kind 落 `"no-error"`——"期望 200 得 500"与"期望 201 得 500"同签名；v1 无消费方可接受，T4/R5 接入时增强。

### 4.4 终端输出（增量）

逐条打印与汇总块**原样保留**（error 形态的既有内联 `[✗] Replay error: {e}` 行自循环 Err 分支移入条目处理内继续打印；`--target` 缺省时条目 url 即原始录制 URL）；失败清单块终端**不脱敏**（与既有内联错误行同暴露面，脱敏只在 AI 桥侧）、error 文本取首行、颜色跟随既有风格、测试断言不依赖 ANSI 色码；汇总块后新增（仅 `failed > 0` 时）：

```text
  Failed entries:
   - #2 POST http://target/login (expected 200, got 401)
   - #3 GET http://target/users (error: connection refused)
```

### 4.5 决策日志（本次会话裁决存档）

| # | 决策 | 备选与理由 |
| :-- | :--- | :--- |
| D1 | PRD 范围从"T3+T6+R4 规则版"收敛为"R6 + T5 搭车 + R3 壳残余" | 双子代理审查证伪原范围：T6/R4 已在 rupost-ai 工作区交付（用户要求增加场景画像与 AI 预留后触发现状重核） |
| D2 | 开发落在 rupost-ai 执行线（非主线） | 用户裁决"在 ai 的 workspace 中继续开发"；FailureContext/ai 管线载体在此线 |
| D3 | 数据结构采方案 A 专用结构体（否决复用 TestResult / 立即接 CLI 两案） | TestResult 空壳字段污染 explain 语义；立即接线超出"预留"裁决。用户补充"复用结构体定义"→ 落为字段形状对齐既有约定（§4.1） |
| D4 | T5 纳入本 PRD + `body=None` 占位导出 | 路线图/矩阵两处搭车承诺；占位保条目可追溯，重放比对不依赖 body。用户裁决 |
| D5 | replay 失败 exit 1 | 用户裁决；CI 消费关键缺口（P-2） |
| D6 | 桥守护用 `mod tests` 单测而非集成测试 | 审查 P1：对齐 `context_from_json_test_report` 先例 + AGENTS.md 纪律 |
| D7 | 合并叙事更正：交付=快进（已 rebase 至 main 尖），交付前二次 `jj rebase -d main` + 全量门禁 | 双审查员交叉实证 `checkpoint.md:266` |
| D8 | `--report json` for replay 不做（serde 留口）；若日后点亮，schema 迁 `src/reporter.rs` 并纳入 `report_schema_test.rs` 锁定 | YAGNI + 序列化契约演进纪律（审查 P2-5） |
| D9 | `body=None` 占位形态 = 复用既有 `From<HistoryEntry>` 的占位标记文本（非空串 `Old snapshot: body not recorded`） | 二轮审查实证既有实现 `model.rs:88-103` 零调用方且填占位文本；复用零新代码、"占位导出"字面吻合、可追溯性更强（用户裁决的占位方向不变，仅形态细化） |
| D10 | `--last` 语义 = 最近 N 条历史**条目**（`HistoryStorage::tail(n)`），CLI help 文案 "runs"→"requests" 顺带修正 | `SelectionStrategy::Last(n)` 实现为 `tail(n)`（`storage.rs:123-127`）；现有 help（`cli.rs:375-376`）与实现语义漂移，对齐同构兄弟 generate 的 "requests"（`cli.rs:422-423`） |
| D11 | 二轮审查（事实核查 + 可实施性双子代理）吸收：引用失实 3 处更正、body=None 口径定案、T5 导出规格化（§4.6）、测试落位与机制定案、Step 4 台账补 DX PRD 状态列 | 见 §4.6 与 §六；原 PRD 的行级代码引用 10/10 核实属实 |

### 4.6 T5 导出规格（`rupost history export`）

- **CLI 面已存在**（`src/cli.rs:372-382`）：`--last <N>`（默认 1）、`-o/--output <FILE>`（required）——本任务**零新增参数**；help 文案 "Number of recent runs" → "requests"（D10，一行顺带改）。
- **导出范围**：`SelectionStrategy::Last(n)` → `HistoryStorage::tail(n)`（`storage.rs:123-127`）取最近 N 条**条目**，追加序（旧→新）即重放执行序。
- **转换**：复用既有 `From<HistoryEntry> for SnapshotEntry`（`model.rs:88-103`，含 `body=None` 占位标记文本，D9）。
- **套件元数据**：`timestamp = Utc::now()`；`source_file = Some("history")`（对齐 `save_batch_snapshot` 先例 `storage.rs:280`）；条目 `id` 沿用历史 id。
- **空历史**：`tracing::warn!` + 不写文件 + exit 0（对齐 generate 先例 `generator/mod.rs:28-31`）。
- **覆盖语义**：`write_snapshot_suite` 静默覆盖（`fs::write`），不新增 `--force`（YAGNI）。
- **确认文案**：`Exported snapshot: {output} ({N} requests)`（镜像 generate）。
- **凭据**：**不脱敏**——导出文件与 `--save-snapshot` 同暴露面，凭据本就原样入快照（§2.1 安全注记覆盖此模型）。

---

## 五、显式非目标

1. **body JSON diff / 比对语义升级（2xx 等价类）不做**——T4 Smart Diff 卡承接，本 PRD 状态码精确相等语义不变；
2. **replay `--report json` 不做**——serde 双向 derive 仅留口（D8）；
3. **AI 归因 CLI 接线不做**——桥函数 v1 无 CLI 消费方，暴露给用户属 fast-follow 立项；
4. **replay 走 R7 钩子/trace 提取不做**——`ReplayExecutor` 不在 `fire_failure_hooks` 管线，v1 范围仅 test 链；
5. **审计留痕随行不做**——X5 护栏引用矩阵 `:48`，落地面另行裁决；
6. **退出码之外的 replay 行为变更不做**——逐条打印格式、快照加载容错语义均保持；
7. **export 导出文件脱敏不做**——与 `--save-snapshot` 同暴露面（§4.6）。

---

## 六、执行计划（jj 原子提交，rupost-ai 执行线）

| 步 | 提交 | 内容 | 验证 |
| :-- | :--- | :--- | :--- |
| Step 1 | `feat(replay): ReplayEntryResult structured per-entry results with exit code` | §4.1 结构体 + `replay_entry` 返回条目（Err→条目收敛，error 形态 `duration_ms` 记实际耗时）+ `ReplayReport.entries` + 失败清单块 + `failed>0` exit 1（落 `cli.rs:770-771` Replay 臂，与 Test 臂同款；`main.rs` 现 17 行，红线无虞零触碰）+ 四项验收测试（混合守恒/stdout 断言/error 归类/0 条目与全失败）——集成测试追加 `tests/snapshot_replay_test.rs`（wiremock 回环），退出码与 stdout 断言用 `std::process::Command` + `CARGO_BIN_EXE_rupost` 子进程（进程内 stdout 被 cargo test 捕获，不可靠）；serde 往返落 `replayer.rs` 新增 `mod tests`；**认领 `verify_features.sh` replay 定位步骤**（mock 起回环端口 + grep `Failed entries:`/退出码，零外网） | `cargo test --workspace` 全绿 + 老测试零破坏（聚合数值断言实证无 stdout/结构锁定） |
| Step 2 | `feat(ai): context_from_replay_entry bridge with redaction` | §4.3 桥 + `mod tests` 决策表 + 三形态 × 脱敏矩阵 + 喂 `infer_possible_causes`/`failure_signature` 端到端（两者皆纯函数，**零 mock**） | clippy `-D warnings` 零告警 |
| Step 3 | `feat(history): wire export command (T5) with placeholder body entries` | §4.6 规格实施：桩锁定测试先行（替换 `tests/history_cli_test.rs:292-308`，**必须加 `RUPOST_HISTORY_DIR` tempdir 隔离**——现状裸跑读写真实历史目录）→ `write_snapshot_suite` + `SelectionStrategy::Last` 接线 + `body=None` 占位导出 + 顺带 R3-壳层测试（追加 `tests/diagnose_integration_test.rs`，可移独立小提交）+ verify_features 链增 export→replay 环节 | 导出→重放闭环集成测试（落 `snapshot_replay_test.rs`） |
| Step 4 | `docs: ledger sync — R6/T5/R3 status and matrix errata` | progress_summary 批次行 + CHANGELOG + checkpoint 措辞勘误（`:254` vs `:266`）+ 矩阵 R3 注记勘误与 `:86`/`:87` 搭车方向张力登记 + **DX PRD R3/R7 状态列**（勿遗漏）+ runbook T3 卡字段口径注记 + README 处置裁决（quick-start 增 `replay`/`history export` 各一行，或记"无对应小节不动"）+ `directory_structure.md` **零新增模块不动**的声明 | — |
| 交付 | — | `jj rebase -d main` 刷新 + `make gate` + `./tests/verify_features.sh` → 快进合并 | 门禁全绿 |

**风险登记**：①main 在交付前再前进 → 二次 rebase 冲突面 = 4 个追加型台账文件 + checkpoint 测试计数，文本级低危（D7）；②Rust 2024 edition / rust-version 1.89，全程**零新增依赖**。

---

## 七、验收指标汇总（机器可验）

| 需求 | 指标 |
| :-- | :--- |
| R6-a | 混合成败（3 条 2 成 1 败，与 SC-A 同规模）单次运行集成测试：`entries.len()==total` 且 count 守恒且失败条目 `request_number` 精确；serde 往返相等 |
| R6-b | stdout 含 `Failed entries:` 块与逐条内容；全成时块不存在；0 条目时聚合全 0 |
| R6-c | 退出码双向断言（exit 0 / exit 1，子进程机制） |
| R6-d | 桥映射决策表全绿（含 query-token 掩码断言）；桥产物喂 `infer_possible_causes`/`failure_signature`（纯函数，零 mock）输出非空 |
| T5 | 桩测试先行失败 → 替换后绿（`RUPOST_HISTORY_DIR` 隔离）；导出文件 replay 加载成功；`body=None` 条目保留且 body 为占位标记文本 `Old snapshot: body not recorded`；空历史 warn+exit 0 不写文件 |
| R3-壳 | `run_diagnose` json 分支 stdout 形态 + 错误路径 `exit(1)` 壳层测试（追加 `tests/diagnose_integration_test.rs`） |
| 链路级 | `verify_features.sh` 增补 replay 定位步骤零外网全绿 |
