# RuPost 开发者 API 调试体验 PRD（全链路 Debug Experience）

> **创建日期**: 2026-10-28 ｜ **状态**: 已立项（需求层；实现项按既有队列裁决落位）
> **对象**: **产品侧**的 API 调试全链路体验（rupost 的用户）——区别于 [Dev Workflow DX PRD](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-27-dev-workflow-dx-prd.md)（开发 rupost 本身的流程体验）
> **父纲与上游**: [AI 总体规划](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-27-ai-master-roadmap.md)（B0–B4 宪法 + 准入条款）、[主执行文档](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-27-master-execution-runbook.md)（队列裁决）、[开发者旅程设计](file:///Users/zsyzzx/project/rust/rupost/.workspace/rupost-ai/doc/plans/2026-10-27-ai-developer-journey-design.md)（S0–S7 旅程）、[E2/E5 PRD](file:///Users/zsyzzx/project/rust/rupost/.workspace/rupost-ai/doc/plans/2026-10-28-ai-failure-triage-and-attribution-prd.md)（D3/D4 实现专项）
> **兜底分级声明（准入强制，per 路线图 §6.1）**: R4/R5 = **B1 增强型**（确定性基线 + AI 增强，旅程 S3/S6）；R8 = **B2+B4**（已立项）；R1/R2/R3/R6/R7/R10 = **确定性需求（B0 领域）**——服务"诊"链但零 AI 参与，是 AI 归因的地基而非附属

---

## 1. 背景与全链路定义

### 1.1 调试链（Debug Chain D0–D7）

开发者的一次 API 调试在时间轴上展开为八环。**全链路需求 = 每一环都有"下一条路"**：任何一环断裂（无工具、无数据、无出口），整条链的价值就淤积在该环。

| 环 | 动作 | 现状（2026-10-28 代码实证） | 缺口 |
| :-- | :--- | :--- | :--- |
| **D0 触发** | CLI 内联 / `t` 文件 / TUI 发起请求 | ✅ 完整（别名全家 + TUI） | — |
| **D1 观察** | 终端高亮响应 / TUI 流式渲染 | ✅ 功能在 | ⚠️ TUI 流式渲染 `state/stream.rs` **0 测试**（R10，测试方案 P1 既有欠账） |
| **D2 初诊** | 网络层 DNS/TCP/TLS/TTFB 拆分 + X.509 | ✅ `diagnose` + `--debug-on-failure` 钩子在 | ⚠️ `--debug-on-failure` **全仓 0 集成测试**（R2，grep 实证）；`diagnose --report json` 契约未锁（R3） |
| **D3 单请求归因** | "为什么这条失败" | 🕳 仅输出 `expected/actual` 原始 diff；终端派竞品 0% 覆盖 | → **R4 `--explain`**（E2，已立项未实现） |
| **D4 批量分诊** | "50 条红里哪几簇" | 🕳 JSON 报告无 `expected/actual`/响应片段（P-3）；replay 只回聚合计数 | → **R5 `ai triage`**（E5，已立项）＋**R6 replay per-entry**（T3） |
| **D5 深挖取证** | API 层归因后追到服务端日志 | 🕳 无出口——k9s 逐 Pod 痛点（本 PRD 的原始诉求） | → **R7 `--on-failure` 钩子 + trace-id 提取**（T6） |
| **D6 修复验证** | 改一处 → 重跑 → 观察 | ⚠️ **CLI 无单请求选择器**（grep 实证无 `--only`/`request_index`）：`rupost t` 只能全文件跑；TUI 可单发但 CI/脚本不可用 | → **R1 单请求重跑**（本 PRD 新立项）＋**R8 `--last` 上下文断言**（P0 PRD 已立项） |
| **D7 沉淀防复发** | 修复成果 → 回归资产 | ✅ generate 语义化/凭据模板化已交付 | → **R9 `ai bootstrap`**（S4 跃迁，下一主线，已立项） |

### 1.2 与既有文档的分工（防重复立项）

| 文档 | 职责 | 本 PRD 关系 |
| :--- | :--- | :--- |
| E2/E5 PRD | D3/D4 的**实现专项**（数据模型、五阶段管线、提交序列） | 本 PRD 引用不复制，只补链路定位与验收视角 |
| P0 上下文/零配置 PRD | D6 的 AI 侧（`--last` 上下文、B4 配置链） | 同上 |
| 旅程设计 | AI 动词（生/翻/炼/诊）在 S0–S7 的落位 | 本 PRD 的"诊"动词给出确定性地基需求（旅程只列了 AI 侧） |
| 主执行文档 | 队列裁决（M1/M2/M3） | 本 PRD 新需求**插槽不翻案**（§6） |
| 完整开发方案 §1.2 | 三问判定准则 | 本 PRD 全部需求已过三关（共性/增益/定位） |

### 1.3 痛点实证（非推测）

- **10–20 分钟/条**的单接口归因、**1–2 小时**的 CI 批量分诊（E2/E5 PRD §1.1 实证）；
- 修复回路断裂：CLI 调试中改完参数只能重跑整个文件（本 PRD grep 实证）；
- `--debug-on-failure` 作为失败初诊钩子，实现存在但**无任何测试守护**（若回归破坏无人知晓）。

---

## 2. 开发者场景 × 链路矩阵

五类画像各走一遍链路，标注每环的体验（✅ 顺畅 ｜ ⚠️ 带病 ｜ 🕳 断裂）：

| 链环 | SC1 后端小李：接口 401 | SC2 QA 小赵：CI 一夜 50 红 | SC3 前端小张：联调契约不符 | SC4 小王：SSE/WS 流式断言失败 |
| :-- | :--- | :--- | :--- | :--- |
| D0 触发 | ✅ 内联 `rupost POST ...` | ✅ `t suite/` DAG 批量 | ✅ mock 契约先行（A5） | ✅ `@websocket`/SSE 剧本 |
| D1 观察 | ✅ 终端高亮 | ✅ 批量摘要 | ✅ | ⚠️ 流式渲染无测试（R10） |
| D2 初诊 | ✅ 网络通/断一键分辨 | ⚠️ CI 无初诊钩子测试（R2） | ✅ | ✅ |
| D3 归因 | 🕳 401 只有 expected/actual → **R4**（规则版即可给出"缺鉴权头/凭据失效"） | 🕳 同左 → **R4** | 🕳 字段缺失无契约级解释 → **R4** ContractMismatch 类目 | 🕳 → **R4** LLM 流式变体（旅程"诊"优先项，借力 A1 空白） |
| D4 分诊 | —（单条） | 🕳 50 条平铺 → **R5** 三簇 + 计数守恒 | — | — |
| D5 取证 | 🕳 查服务端日志要切 k9s → **R7** 钩子直通 stern | 🕳 同左（聚类后仅对根因簇取证） | — | — |
| D6 修复验证 | 🕳 改完 token 重跑要跑全文件 → **R1** `#3`/`--only` | ⚠️ 重跑失败子集 → R1 + 报告定位 | ✅ 改契约重跑 mock | 🕳 单帧重放受限（现有剧本重跑覆盖） |
| D7 沉淀 | ✅ `g` 模板化/语义命名；🔧 bootstrap 语义增强（R9） | ✅ 同左 | ✅ 契约即 mock（A5 保持） | ✅ 剧本已沉淀 |

**矩阵结论**：D3/D4/D5 是三条断裂带（AI 增强型，已有实现专项）；**D6 的单请求重跑是本次新识别的确定性断裂**（一行命令的缺失让修复回路在 CLI 侧不可用）；D2 带病（功能在、守护无）。

---

## 3. 逐环需求汇总（R 清单）

| # | 链环 | 需求 | 兜底级 | 状态/落点 | 优先级 |
| :-- | :-- | :--- | :-- | :--- | :-- |
| **R1** | D6 | **CLI 单请求重跑选择器**：`rupost t api.http#3`（序号）与 `--only <n\|@name>`；只执行该条（不走 DAG），`--var`/env 级联照常；越界可读报错 | 确定性 | **本 PRD 新立项**（grep 实证缺口） | **P1 · M1** |
| **R2** | D2 | `--debug-on-failure` 失败自动诊断集成测试（不可达端口 → 输出 DNS/TCP 拆分，连接失败时 TTFB 段优雅降级） | 确定性 | 测试方案 P1 既有欠账，本 PRD 并入调试链视角 | P1 · M1 |
| **R3** | D2 | `diagnose --report json` 输出契约测试（与 §5.18 schema 纪律同守卫） | 确定性 | 测试方案 P1 既有 | P2 · fast-follow |
| **R4** | D3 | `rupost test --explain`：规则版 `[Possible Causes (Rule)]` 基线 + AI 根因（Top-3 熔断；LLM 流式响应变体优先） | **B1** | E2/E5 PRD Step 1–3 | P1 · **M1 已含** |
| **R5** | D4 | `rupost ai triage` / `--triage`：签名桶守恒聚类（100% 计数守恒、O(M) Token、`--json`）＋前置的强类型报告与 `--report-file` | **B1** | E2/E5 PRD Step 0–2 | P1 · **M2 已含** |
| **R6** | D4 | replay per-entry 结构化结果（`ReplayEntryResult`）——快照重放的"哪条失败"定位；与 R5 报告结构共享 | 确定性 | runbook T3 | P2 |
| **R7** | D5 | `--on-failure <cmd>` 钩子（`tokio::process` + timeout）＋**响应头 trace-id 自动提取**（Traceparent/X-Request-Id/X-Trace-Id → `RUPOST_FAILED_TRACE_ID`；缺失时空串 + warning）——k9s 痛点的最终形态：rupost 给锚点，聚合委托 stern | 确定性 | runbook T6（本 PRD 补充 trace 提取协议） | P2 |
| **R8** | D6 | `--last` 上下文断言（最近响应注入归因）＋零配置链（env > ai.toml > Ollama 探测） | **B2+B4** | P0 PRD 已立项 | P1 · **M1 已含** |
| **R9** | D7 | `ai bootstrap --from history`（调试成果 → 语义增强回归资产，S4 价值跃迁） | **B1** | bootstrap 提案已立项 | P1 · **M2 已含** |
| **R10** | D1 | TUI 流式渲染（`state/stream.rs`）单测补齐 | 确定性 | 测试方案 P1 既有 | P2 · fast-follow |

---

## 4. 新增需求用户故事（GWT）

### US-R1：单请求重跑（修复回路的确定性地基）

> As a CLI 调试的开发者, I want `rupost t api.http#3` 只重跑第 3 条, So that 修一处验证一处而不用跑全文件。

- Given `api.http` 含 5 条请求，When `rupost t api.http#3`，Then **仅执行第 3 条**（不展开 DAG），终端只含该条结果，退出码反映该条成败；
- Given 第 1 条声明 `# @name login`，When `rupost t api.http --only login`，Then 仅执行该条；
- Given `--only` 同时收到序号与名字或越界序号（`#9`），When 执行，Then 单行可读错误（列出合法范围/可用 `@name`），**非 panic**、exit 1；
- Given 该条引用未捕获变量（上游未跑），When 执行，Then 走既有"未定义变量警告 + 字面发送"语义并显式 warning 提示（与批量语义一致，不静默）；
- And 表驱动测试锁定解析（`#n`、`--only n`、`--only @name`、越界、重名 `@name` 取首个 + warning），进入 `src/cli` 表驱动套件。

### US-R7：失败钩子的 trace 锚点协议（k9s 痛点最终形态）

> As a 后端排障工程师, I want 失败时钩子环境变量里直接有 trace-id, So that `--on-failure 'stern ... --grep $RUPOST_FAILED_TRACE_ID'` 一条配置接通服务端日志。

- Given 响应头含 `Traceparent`/`X-Request-Id`/`X-Trace-Id` 任一，When 该请求失败且配置了 `--on-failure`，Then `RUPOST_FAILED_TRACE_ID` = 提取值（多候选按此优先级取首个）；
- Given 响应头无任何 trace 头，When 失败，Then 该变量为空串且 `TestResult.warnings` 追加一条"未检测到 trace 头，钩子仅暴露 url/status"；
- And 提取值同样进入 `--explain` 的 `FailureContext`（与 E2/E5 的 DTO 复用，供 AI 归因参考）；
- And 钩子进程超时/非零退出：stderr 透传、不阻塞 Fail-Fast（T6 卡既有验收不变）。

### US-R2：失败初诊的守护测试（带病功能补防）

- Given wiremock 指向一个**不可达端口**，When `rupost t x.http --debug-on-failure`，Then 失败输出包含 DNS/TCP 拆分段，连接失败场景不输出 TTFB 段且不 panic；
- Given 请求成功，When 同参数执行，Then 无诊断输出（零噪音）。

---

## 5. 显式非目标

1. **运行期零 AI**（B0 宪法）：R1/R7 等"诊"链地基永远确定性实现，AI 只在执行结束后介入（E2/E5 模式）；
2. **不内建 kubectl/日志平台聚合**：R7 只暴露锚点数据，聚合展示委托用户管道（T11 探路维持，翻案条件不变）；
3. **不做调试聊天面板/对话入口**（旅程反模式清单 §5）；
4. **AI 不改写测试文件**：全部诊断建议只读输出；
5. **不依赖 Makefile 的产品功能**（DX PRD 裁决：make 仅服务开发侧）；
6. **不做 HTML 报告**（backlog 维持）。

---

## 6. 里程碑落位（插槽不翻案 2026-10-27 裁决）

| 里程碑 | 既有内容 | 本 PRD 插槽 |
| :--- | :--- | :--- |
| **M1** 地基与即时效率 | P0（R8）+ import curl + E2（R4） | **+ R1 单请求重跑**（~1 天，纯确定性，修复回路的最高频断点）**+ R2 测试债**（0.5 天）——两者是 M1 全部 AI 功能的"确定性对照面" |
| **M2** 批量与认知 | E5（R5）+ bootstrap（R9） | R6（T3）与 R5 的强类型报告同批共享结构 |
| **M3** 供给侧 | MCP MVP | 不变；MCP 的 `get_history`/`run_request` Tool 将直接消费 R1/R6 的能力面 |
| fast-follow | — | R3 / R10（测试契约债，随相邻改动顺带） |

**排序理由**：R1 与 R2 不与任何 AI 项竞争资源——它们恰好构成 E2/E5 的**降级基线的用户侧兄弟**（AI 缺失时，开发者靠 `#3` 重跑 + `--debug-on-failure` 初诊仍能完成 80% 修复回路），这是 B1 兜底哲学（"AI 是增强，不是依赖"）在确定性侧的落实。

---

## 7. 验收与度量（阶段成功指标）

| 需求 | 机器可验指标 |
| :--- | :--- |
| R1 | 表驱动解析套件 + `#3`/`--only` 集成测试全绿；越界零 panic |
| R2 | 不可达端口场景输出含时延拆分段；成功场景零诊断噪音 |
| R4/R5 | E2/E5 PRD §7 各 Step 指标（熔断/守恒/降级三路径） |
| R7 | 假脚本捕获 `RUPOST_FAILED_TRACE_ID` 断言；无 trace 头时 warnings 可见 |
| 链路级 | SC1 场景端到端走查（401 → `--explain` → `#3` 重跑 → `g` 沉淀）在 `verify_features.sh` 新增步骤下零外网全绿 |
