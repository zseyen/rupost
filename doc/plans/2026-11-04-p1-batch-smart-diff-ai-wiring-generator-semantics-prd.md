# P1 批次 PRD：T4 Smart Diff + AI 桥接线 + T1/T2 生成语义线

- **状态**：Proposed（待评审）→ Accepted → In Progress → Done
- **日期**：2026-11-04
- **执行队列**：runbook P1 线（T4 取证链收官 + AI 接线 fast-follow 解冻 + T1/T2 生成语义批次）
- **范围基线**：rupost-ai 执行线（main = edfa8da2，已推送 debug-chain-r6）
- **关联文档**：[快照路线图](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-07-01-snapshot-replay-roadmap.md) 条目 2（T4 设计源头）、[生成语义执行计划](file:///Users/zsyzzx/project/rust/rupost/.workspace/rupost-ai/doc/plans/2026-10-27-generation-semantics-execution-plan.md) §1c/§4（T1/T2 既定设计）、[R6 PRD](file:///Users/zsyzzx/project/rust/rupost/.workspace/rupost-ai/doc/plans/2026-11-04-debug-chain-r6-replay-per-entry-prd.md)（body diff/snippet 显式留给本批）
- **事实源分工**：本文件只承载需求与规格；进度唯一事实源是 progress_summary.md。
- **兜底分级**：T4/AI 接线 = B0+B1（比对确定性，AI 只读增强）；T1/T2 = B0 确定性（机械降级路径，AI 增强归 bootstrap）。
- **分阶段交付声明**：Phase 1 = T4 + AI 接线（调试链收官，一个 PR）；Phase 2 = T1 → T2（生成语义线，独立批次）。

---

## 一、背景与痛点（实证）

| # | 痛点 | 证据 |
| :- | :--- | :--- |
| P-1 | replay 只比对状态码：body 行为变化（状态码不变、字段变了）完全不可见——SC-3 联调场景最大盲区 | `replayer.rs:136` 仅 `expected==actual`；R6 PRD §4.2 明示"body diff 归 T4" |
| P-2 | 抖动字段假失败隐患：timestamp/uuid/request_id 类字段一旦纳入比对，业务正确也报错 | 路线图条目 2 需求背景原文 |
| P-3 | AI 归因对重放场景不可感知：桥已建（`context_from_replay_entry`）但无 CLI 消费方，replay 失败只有原始 diff | R6 PRD §三 R6-d"解冻条件见 fast-follow" |
| P-4 | 生成的用例重跑语义错误：历史中已解析的动态值（UUID/时间戳）被固化为字面量，重放发旧值 | T1 卡（执行计划 §1c）；bootstrap 机械基线同源问题 |
| P-5 | 链式数据流被打平：登录产出 token → 下游注入的关系在导出时丢失，生成文件不可直接重跑 | T2 卡（§4 问题本质）；SC-1/SC-2 沉淀场景依赖 |

## 二、用户故事（GWT，关键三条）

**US-T4**（小张·联调）：Given 快照录制自旧版服务（body 含 `timestamp`/`request_id` 抖动字段），When `replay snap.json -t <新环境>`，Then 状态码 + body（排除抖动字段后）双重比对；抖动差异不假失败、真实字段缺失/值变被抓并定位到条；`--status-only` 可退回 R6 语义。

**US-AI**（小李·开发）：Given replay 有失败条目，When `replay --explain`，Then 失败条目下方输出规则版 `[Possible Causes]`，配置 AI 后按签名去重追加根因（熔断/降级语义与 `t --explain` 一致），退出码仍由比对结果决定。

**US-T1/T2**（小赵·沉淀）：Given 历史含登录→下游链路（token 在 body/Authorization），When `g --last N`，Then 动态值按形状模板化（`$uuid`/`$timestamp`），链式值自动重建（上游 `@capture` + 下游 `{{}}`），生成文件直接 `rupost t` 跑通。

## 三、需求清单

| # | 需求 | 验收标准 | 兜底 |
| :- | :--- | :--- | :-- |
| **T4-a** | body JSON 比对（深比较语义定案表）：双侧解析为 `serde_json::Value` 递归比较——**数组序敏感**（serde_json 原生，排序抖动=差异，排序选项登记非目标）、**int/float 判不等**（`1`≠`1.0`，序列化漂移属已知限制登记 backlog）、**Unicode/转义形态天然归一**（parse 后比较）；双侧任一 >256KB → 跳过（`body_matched=None`）；**skip 条件拆分**：仅"快照 body 占位文本/缺失"跳过；双侧均解析失败但原文不等 → 计 body 失败（单行 `body changed (non-JSON)`）——关闭 200→200 HTML 换 JSON 盲区 | ① 抖动字段双侧剔除后相等 → matched；② 真实差异 → body 失败 + diff 证据；③ 占位/缺失/超限 → `body_matched=None` 回退 R6；④ 状态码不等时跳过 body（先比状态码）；⑤ verify_features 12/12.5 回归保持绿（占位跳过分支为承重墙） | B0 |
| **T4-b** | `ReplayEntryResult` 证据扩展：`body_matched: Option<bool>` + `body_diff: Option<String>`（**前 3 处差异、每处一行 `path: expected → actual`**、经 `ai::redactor` 脱敏 + `truncate_utf8_safe` 512B 截断）+ `response_body_snippet: Option<String>`（实际响应片段，**采集点必经 `redact_and_truncate_body` 脱敏**——直上 LLM 的 Gap Review P3 不变量）。**第四形态**：状态匹配 + body 失败——逐条打印 `[✗] Body mismatch: <首处差异>`；Failed entries 块第四臂 `- #N M U (body: <path>: expected → actual)`（现三臂会打印 `expected 200, got 200` 荒谬行，`replayer.rs:117-140` 需扩臂）；R6 §4.2 形态表扩充第四行；body-only 失败计入 failed → exit 1（专项测试断言） | serde 往返 + 缺省回填；4 处字面量构造点机械更新（replayer.rs:226/270/288、rules.rs:574） | B0 |
| **T4-c** | 排除列表：默认内置 14 键（剔除裸 `date`/`time` 高业务误伤键：`timestamp/created_at/updated_at`、`uuid/request_id/requestId/trace_id/traceId/span_id`、`token/session_id/sessionId/access_token/refresh_token`）；`--ignore-fields <KEY,...>` 追加（**大小写敏感**精确匹配、不命中路径静默）；匹配 = **叶子键名精确相等、双侧同时剔除**（对象递归 + 数组内对象）；顶层标量/标量数组/数组长度变化无键上下文 = 不可排除 = 真实差异（决策表补三行锁定）；**明示风险**：被排除键上的真实回归被掩盖（假通过面），指南 §6 增提示；`--status-only` 整体停用（恢复 R6 语义） | 决策表（嵌套命中/数组内对象/顶层标量/数组长度/大小写/双侧剔除） | B0 |
| **T4-d** | toml 全局配置不做（ConfigLoader 现无 `[replay]` 节，YAGNI——CLI flag 足够 v1；登记 backlog） | — | — |
| **AI-a** | `replay --explain`：对失败条目（按 failure_signature 去重，Top-3 配额 + 连续 2 错熔断）输出规则版 `[Possible Causes]`；AI 可用时经 `FailureAttributor`（B4 链 `ai::config::resolve`）追加根因段。**桥证据合成（P0）**：body 失败条目合成 `FailedAssertionInfo { raw: "body deep-compare <path>", expected/actual ← body_diff }`——使 `failure_signature` 摆脱 `no-error` kind（承接 R6 §4.3"T4 接入时增强"义务，防不同漂移合并归因）、规则版命中既有断言臂而非误导兜底文案、AI prompt 看到证据。**入口重构（登记）**：`ExplainSession` 公共方法 TestResult 硬绑定（explain.rs:33/76/90），新增 FailureContext 级入口 `explain_context_with_client`，渲染复用私有 `print_deterministic_causes` 不复制格式 | 无 Key 降级：规则版 + 黄色提示 + exit 不变；verify_features 步骤 13 四条锁法（对照步骤 8：exit 1 + `Possible Causes` 出现 + `AI Explanation` 不出现 + 全成功无归因块） | B0+B1 |
| **T1** | 动态值反向识别（**语法勘正**）：产物为 `{{$uuid}}` / `{{$timestamp}}`（**双花括号包裹**——resolver 仅解析 `{{}}` 内 `$` 前缀，裸 `$uuid` 原样发送）；映射形状收缩 + 内置扩容双轨：UUID v4 → `{{$uuid}}`、epoch 秒 → `{{$timestamp}}`（既有内置）；**ISO8601 → `{{$timestamp_iso}}`、epoch 毫秒 → `{{$timestamp_ms}}`（resolver 新增两内置分支，保形态重跑）**；覆盖 Header 值 / Body JSON 字符串叶子 / URL query 值；**凭据模板化优先定序**：敏感键值凭据模板化胜出，T1 扫描跳过敏感键；同值多处逐出现独立求值（**已知限制**：无单值 hoist 设施，登记非目标）；短值/不匹配原样保留 | 表驱动（UUID/ISO8601/epoch 秒与毫秒/短值不误伤/敏感键跳过/新内置分支）；集成：生成文件重跑成功且两次运行值不同（三形状各验） | B0 |
| **T2** | 链式值溯源（§4 既定设计 + **两处 P0 定案**）：①capture 产物语法 = 现行 **`body.<path>`**（§4 原文 `body.json.<path>` 与 parser 冲突——`capture.rs:66` strip_prefix("body.")，`json.` 会被当键名）；②**生成管线优先级：T2 溯源改写 > T1 形状模板化 > 凭据模板化，被溯源命中值豁免凭据模板化**（否则 Bearer 值先被 `{{authorization}}` 吞掉，链路断、`rupost t` 直接跑通不可达）；时间序遍历，下游高熵候选（≥8 字符）反序搜上游响应（**反序就近先得 = 就近消歧，tie 行决策表锁定**），scheme 前缀剥离；Cookie 链**删头是对现行 Cookie→`{{cookie}}` 模板化的显式例外**，单次运行靠 ephemeral 共享罐（executor AutoPersist）、跨运行需 `--cookie-file`；派生型（Basic/HMAC）不识别 | 五决策表 + tie 行（单命中/多命中消歧/熵不足跳过/前缀剥离/Cookie 特判）+ wiremock 链式闭环：生成两请求文件直接 `rupost t` 跑通 | B0 |

## 四、关键设计决策

| # | 决策 | 备选与理由 |
| :- | :--- | :--- |
| D1 | body 比对**默认开启**，`--status-only` 逃生舱 | 默认关会让 T4 成死功能；R6 语义保留通道 = 逃生舱而非默认。行为变更登记 CHANGELOG Breaking 候选 |
| D2 | 排除匹配 = 叶子键名精确相等（递归遍历对象/数组），不做 JSONPath 前缀 | 路线图原文"忽略特定的动态键"；JSONPath 复杂度不成比例，登记非目标 |
| D3 | 状态码不等时**跳过** body 比对（避免双重报错噪音）；状态相等才进 body | 单一归因原则：先传输层后内容层 |
| D4 | `matched` 语义升级为 status+body 联合；error 字段保持传输层语义不变（R6 §4.2 契约不动），body 差异走独立 `body_diff` 字段 | 不破坏 R6 三形态映射表与既有测试 |
| D5 | AI 接线只挂 `replay --explain`（`ai triage` 消费 replay 失败属 E5 报告面，不在本批） | 报告落盘/聚类是另一条契约线；YAGNI |
| D6 | T1 值扫描器落 `src/generator/dynamic.rs` 独立模块；T2 溯源落 `src/generator/trace.rs`，复用 T1 扫描器产出候选 | 共享管线不共文件；T2 依赖 T1 先行（runbook 既定） |
| D7 | T2 v1 只做单文件链（§4 产物形态 A）；多文件 DAG（形态 B）与 curl 注释（形态 C）登记非目标 | 形态 B 复杂度全域，有真实需求再立项 |

## 五、非目标

1. JSONPath 前缀式排除语法、toml `[replay]` 配置（D2/D4）；
2. `ai triage` 直接消费 replay 失败（E5 报告契约线）；
3. T2 多文件 DAG 与 curl 链注释（D7）；派生型依赖（Basic/HMAC）机械识别；
4. body diff 的逐字段 JSON Patch 格式（v1 摘要文本即可，结构化 diff 登记 backlog）；
5. 语义化命名增强、`--interactive` 组合（生成语义计划既有 backlog 项）。

## 六、执行计划（jj 原子提交，rupost-ai 线）

| 步 | 提交 | 内容 | 验证 |
| :- | :--- | :--- | :--- |
| P1-S1 | `feat(replay): smart diff — body comparison with dynamic-field exclusion` | T4-a/b/c：deep-compare + 排除列表 + `--ignore-fields`/`--status-only` + 证据字段 + 终端摘要行 + 决策表/往返/集成测试（wiremock：抖动不假失败/真实缺失被抓/非 JSON 回退/占位 body 回退/状态码优先） | workspace 全绿 + 老测试零破坏（R6 状态码语义测试不动） |
| P1-S2 | `feat(replay): --explain wiring — rule baseline + AI attribution on failed entries` | AI-a：桥消费 + ExplainSession 复用 + 降级契约测试（对照 t --explain）+ verify_features 步骤 13 | 无 Key 降级集成测试 + clippy 零告警 |
| P1-S3 | `feat(generator): dynamic value reverse templating (T1)` | T1：形状扫描器 + 三处覆盖 + 表驱动 + 集成（重跑值不同） | 表驱动全绿 + bootstrap 基线联跑 |
| P1-S4 | `feat(generator): chain value tracing (T2)` | T2：溯源纯函数 + 五决策表 + wiremock 链式闭环 | 生成两请求文件直接 `rupost t` 跑通 |
| P1-S5 | `docs: ledger sync` | progress_summary 批次行 + CHANGELOG（D1 用 **Breaking:** 前缀——exit 码翻转面 + `--status-only` 逃生，先例 CHANGELOG.md:69）+ 矩阵 T4/T1/T2 翻正 + runbook 卡注记 + roadmap 条目 2 勘误注记（JSONPath/toml 非目标化）+ **README、checkpoint、directory_structure.md（新增 generator/dynamic.rs、generator/trace.rs）、DX PRD 状态列** + 指南 §6（新语义/排除盲区提示）/§8/§11 | — |
| 交付 | — | rebase -d main + make gate + verify_features → bookmark 前移推送 | 门禁全绿 |

## 七、验收指标（机器可验）

| 需求 | 指标 |
| :- | :--- |
| T4 | 抖动排除后相等→matched=true；真实缺失→failed+body_diff 非空；非 JSON/占位 body→body_matched=None；`--status-only` 恢复 R6 行为；serde 往返；exit 码语义随 matched |
| AI | 无 Key：规则版输出 + 黄色提示 + exit 不变；签名去重（同签名只归因一次）；Top-3 后提示 | 
| T1 | 表驱动 ≥6 决策行；集成：两次生成 UUID/时间戳不同 |
| T2 | 五决策表全绿；wiremock 链式闭环：`g` 产物 `rupost t` 直接跑通 |

## 八、决策日志（双子代理评审裁决存档，2026-11-04）

**评审结论**：骨架健全（依赖方向/复用面/挂载点全部属实）；**1 项实测利好——S1 默认开启 body 比对经逐测试核对仓库内零翻红**（既有测试 body 均非 JSON 或逐字节恒等；demo 三请求响应体静态；verify_features 12/12.5 依赖占位跳过分支为承重墙已纳入回归）。6 项 P0 全部吸收进需求表（T1 语法勘正/snippet 脱敏/skip 拆分/第四形态/桥证据合成/T2 语法与管线顺序）。

| # | 裁决 | 理由 |
| :- | :--- | :--- |
| R1 | D1 默认开启接受（本线未发布，无存量用户面；CHANGELOG 用 Breaking: 前缀登记翻转场景与逃生舱） | 默认关=T4 死功能；仓库有凭据模板化反转契约先例 |
| R2 | 排除列表 14 键：剔除裸 `date`/`time`（预订 date 属真实业务字段，静默豁免=假通过） | 叶子精确匹配下 `token` 不误伤 `token_type`；裸键误伤面不对称且无反向收窄机制 |
| R3 | body_diff = 前 3 处、每处 `path: expected → actual` 一行、512B 截断；path 记法 `users[2].id` 点号+下标 | 单行首处对全局翻转信息量为零；3 处区分局部漂移与全局翻转；结构复用为桥合成断言 expected/actual（一鱼两吃） |
| R4 | T2 tie-break = 反序就近先得，决策表补 tie 行锁定遍历方向 | "反序搜上游"实现下先命中即最近，无需额外规则；锁行防未来正序化静默变语义 |
| R5 | 深比较：数组序敏感（serde_json 原生）、int/float 不等——均登记已知限制非目标 | 归一化/排序选项复杂度不成比例；serde_json 1.0.149 实证探针支撑 |
| R6 | 大 body 256KB 上限跳过（body_matched=None）；T1 同值多处逐出现独立求值登记已知限制 | 比较成本上界；无单值 hoist 设施（T2 溯源值是唯一 hoist 例外） |
| R7 | Phase 2 内 S3→S4 顺序原子提交（T2 语法/顺序两 P0 已在 PRD 定案，无需拆 bookmark） | 评审建议的拆分前提（T2 规格空洞）已消除 |

## 九、开放问题

（全部于评审收口，见 §八；无遗留。）
