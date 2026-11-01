# RuPost 主执行文档：会话汇总与任务队列 (Master Execution Runbook)

> **日期**: 2026-10-27
> **性质**: 本次会话全部工作的汇总 + **可直接执行的任务队列**。每个任务卡含目标 / 前置 / 实施要点 / 验收标准 / 量级，任何执行者（人或智能体）可按卡冷启动。
> **事实源约定**: 本文件是任务队列的**活状态载体**；[完整开发方案](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-27-complete-development-scheme.md)（需求分析与文档治理层）、[生成语义执行计划](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-27-generation-semantics-execution-plan.md)（决策与设计记录）、[future_backlog.md](file:///Users/zsyzzx/project/rust/rupost/doc/plans/future_backlog.md)（显式取舍）、[progress_summary.md](file:///Users/zsyzzx/project/rust/rupost/doc/plans/progress_summary.md)（进度登记）。
> **当前基线**: `5985ab44`，全量 29 测试二进制 0 失败，fmt/clippy 双零。

---

## 一、当前状态快照

### 1.1 本次会话交付的提交栈

| 提交 | 内容 |
| :--- | :--- |
| `dc1f048c` | **fix(generator)**: `rupost g` 凭据泄露 P0 修复（复用 exporter redact 语义；反转把泄密锁成契约的旧测试） |
| `cfad6af3` | **feat(generator)**: 凭据模板化为 `{{变量}}`（Header 保留 scheme 前缀；query 字符串级重组防花括号重编码；顶部变量清单 + CLI 提示行）。确立分工：export `[REDACTED]` 给人看 / generate `{{}}` 给机器跑 |
| `340bd0cf` | **feat(generator)**: 语义化命名（`### GET /users` + `@name get_users`，冲突序号去重，query 不入名）+ `--templatize-host`（origin → `{{base_url}}`，默认行为不变），e2e 闭环含 CLI 提示行断言 |
| `42b3fc08` | **docs**: 执行计划落盘 + future_backlog 条目 7–12 + progress_summary 登记 + CHANGELOG + 快照路线图勘误 |

### 1.2 决策记录（一句话版）

| 议题 | 裁决 | 依据 |
| :--- | :--- | :--- |
| 云调试 / 多服务日志聚合 | 降级为通用 `--on-failure` 钩子 + 零成本探路 | 对抗审查（需求 4 / 技术 5 / 商业 3 分） |
| 生产环境平台日志 adapter | **Won't do（当前）** | 商业 3 分：与变现阶梯错配；翻案条件 = 阶梯二付费钩子 |
| kubectl 委托聚合 | 推迟；三实验前置 | 多行合并空转等格式假设未验证 |
| 录制 T4（代理抓包） | **Won't do（当前）** | HTTPS MITM 是 Reqable/Charles 重战场 |
| 录制 T2（export 转实） | 推迟，随 Phase 2a 搭车 | 与 `generate` 重叠；前提已被代码推翻 |
| 录制 T3（cURL/HAR 导入） | 有条件做，cURL 先行 | A3 Markdown 叙事延伸 |
| 链式请求导出 | 设计定稿（值溯源），切片 1d 待排 | 见执行计划 §4 |
| 战略排序 | **A1 激活 → junit → 分发** 先于扩展项 | 竞争复审 §4 最高杠杆 |

### 1.3 已证伪假设（翻案必须先做实验）

1. **闭环数据流**：trace-id 必须取自重放后的新 `TestResult.response.headers`；快照只是 replay 输入源与预期基线，不是 trace 提供者。
2. **CRI 运行时对多行日志每行独立打时间戳**——按"行首无时间戳"做多行合并的方案在真实 kubectl 输出上空转。
3. **响应头回传 trace-id 依赖网关配置**（X-B3 系是请求传播头）——未埋点环境整体静默失效。
4. **快照路线图 P1-4"history 不带 Body"过时**——`recorder.rs` 早已全量存 body 且带兼容层（已勘误）。

### 1.4 每任务通用门禁（提交前三步，缺一不提交）

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test                       # 全量 0 失败
```

纪律：`jj` 原子提交，功能 + 测试同一提交；核心引擎改动追加 `examples/run_all.sh` 与 `tests/verify_features.sh`。

---

## 二、任务队列（按建议执行顺序）

> P0 = 立即（安全/审计）；P1 = 生成语义收尾 + 战略杠杆；P2 = 取证链路；P3 = 生态件；并行 = 零成本探路。

### T10 · TUI 历史还原路径凭据审计 — P0 · 0.5 天 · **下一个动作**

- **目标**：确认 TUI 历史面板 Enter 反向还原（`format_request_snapshot_to_http`，TUI 独立实现，未复用 generator 逻辑）是否存在第二个凭据明文面。
- **要点**：读该函数对 `Authorization`/`Cookie`/敏感 query 的处理；若泄密 → 决策取舍（TUI 会话草稿保留真值 vs 同步 `{{}}` 模板化）并同修；若不泄密 → 登记显式结论到 future_backlog 条目 12 并关闭。
- **验收**：审计结论落档；若有修复则含对抗性回归测试同提交。

### T1 · 动态值反向识别（Phase 1c）— P1 · 2–3 天

- **目标**：历史中已解析的动态变量值，生成时按形状反向模板化——`8f9c2d...-...`（UUID v4）→ `$uuid`、ISO8601 / epoch → `$timestamp`，使生成用例重跑时语义正确（新 UUID / 新时间戳）。
- **要点**：值扫描器覆盖 Header / Body（JSON 字符串叶子）/ Query 三处；与凭据模板化共用遍历管线；形状不匹配的值原样保留；同值多处出现只识别一次。
- **文件**：`src/generator/http.rs`（扫描 + 替换）+ `mod tests` 决策表。
- **验收**：表驱动测试（UUID / ISO8601 / epoch 秒与毫秒 / 普通短值不误伤 / 同值多位置）；集成测试：生成文件重跑成功且两次运行值不同。
- **复利**：本任务即 [ai bootstrap 提案](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-26-ai-bootstrap-proposal.md)的机械降级路径，A1 激活后 AI 增强直接叠加。

### T7 · A1 激活（`rupost ai assert`）— P1 · 2–3 天 · 战略最高杠杆

- **目标**：终结"crate 在仓无引用方"的最大战略-代码断点，把 A1 从"代码在仓"变"用户可感知"。
- **要点**：按 [AI 激活 PRD](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-26-ai-activation-prd.md) 方案 A——1 行依赖 + ~40 行 CLI 接出 `rupost ai assert "<自然语言>" [--raw]`；环境变量密钥不入盘不入日志；`examples/ai_assert/` 无真实 Key 离线 dogfooding；crate client 0→4 测试。显式裁剪 `@ai_assert` 运行时指令。
- **验收**：PRD 功能点全绿；progress_summary 状态从"待实施"翻正。

### T2 · 链式值溯源（Phase 1d）— P1 · 2.5–3.5 天

- **目标**：链式请求（登录 → 下游用 token）导出时自动重建数据流：上游 `# @capture token from body.json.data.access_token`，下游 `Bearer {{token}}`。
- **要点**：设计定稿见[执行计划 §4](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-27-generation-semantics-execution-plan.md)——高熵过滤（≥8 字符防 `1`/`true` 误报）、就近消歧（历史时间序即拓扑序）、scheme 前缀剥离、Cookie 链删头靠罐（匹配上游 Set-Cookie 时省略 Cookie 头）、派生型（Basic/HMAC）不机械识别→人工变量兜底。
- **前置**：建议在 T1 之后（共享值扫描管线）。
- **验收**：五张决策表（单命中/多命中/熵不足/前缀剥离/Cookie 特判）+ wiremock 链式闭环：生成的两请求文件直接 `rupost t` 跑通。

### T3 · replay per-entry 结构化结果（Phase 2a）— P2 · 1–2 天 · 取证承重墙

- **目标**：`ReplayReport` 扩出 `Vec<ReplayEntryResult>`（快照引用、新状态码、比对结论、耗时），失败可定位到条。
- **文件**：`src/runner/engine/replayer.rs`（现仅聚合 `total/passed/failed`）+ `tests/snapshot_replay_test.rs`。
- **验收**：多请求一败一成的快照重放，报告能分辨具体条目；既有聚合输出语义不变（老测试全绿）。

### T5 · `export` 桩转实 — P2 · ~20 行 · 随 T3/T4 搭车

- **前置决策（分钟级，先拍板）**：旧历史条目 `body=None` 的导出语义——跳过该条 or 占位导出。
- **要点**：`main.rs:67` 桩转实，复用 `write_snapshot_suite` + `SelectionStrategy::Last`；桩行为锁定测试先行失败驱动替换。

### T4 · Smart Diff（Phase 2b）— P2 · 2–3 天 · 依赖 T3

- **目标**：replay 比对从仅状态码升级到 body JSON diff + 动态字段排除列表（`timestamp`/`uuid`/`trace_id`/`token` 等，见[快照路线图条目 2](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-07-01-snapshot-replay-roadmap.md)），支持 `--ignore-fields`。
- **验收**：抖动字段不假失败（排除列表生效）+ 真实字段缺失被捕获（diff 生效）两组测试。

### T6 · `--on-failure` 通用钩子（Phase 2d）— P2 · 1–2 天 · 依赖 T3

- **目标**：失败时以环境变量（`RUPOST_FAILED_URL` / `_STATUS` / `_TRACE_ID` 等）执行用户命令——日志聚合让用户自行接 stern 管道，不绑 kubectl。
- **要点**：**必须 `tokio::process::Command` + `tokio::time::timeout`**（技术审查教训：std 阻塞调用会占死 worker 且无超时）；stderr/非零退出码用户可见；不阻塞 Fail-Fast 语义。
- **验收**：wiremock 失败用例触发钩子（假脚本记录环境变量）集成测试；三平台 CI 绿。

### T8 · `--report junit` — P3 · 1–2 天

- **目标**：CI 生态接入件，放大"回归引擎"定位。
- **要点**：BatchExecutor JSON 报告侧新增 junit XML 渲染；schema 对照 Jenkins/GitHub 消费格式；输出契约测试（照 §5.18 schema 纪律）。

### T9 · 分发 — P3 · 工程事务 · **2026-10-28 扩展容器与云分发**

- cargo install（crates.io 发布清单）→ Homebrew tap → release 二进制（评估 cargo-dist）→ **容器镜像（multi-arch、non-root，云原生 Job/CronJob 与 GitOps hook 的载体）→ devcontainer feature → 官方 CI Action 仓库（随 T8 junit）**。竞争复审 C 层"发版前必补"；云原生介入与云开发接入的最小载体（分析见[能力与云接入总览](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-28-capability-cloud-integration-review.md) §2.3/§3.4）。

### T11 · 零成本探路 — 并行 · 0 开发

- **文章**：《从一次 API 测试失败到跨服务日志取证：stern --grep、Grafana Trace-to-logs 与终端回归引擎的差距》投 Show HN / r/kubernetes，反响决定 backlog 条目 9 是否翻案；
- **集群三实验**（结果写入 future_backlog 条目 9）：①CRI 多行日志时间戳实际形态；②目标服务 trace 回传现状；③`kubectl logs -l --all-containers --prefix --timestamps` 组合行为与最低 kubectl 版本。

### Backlog 卡（有真实抱怨再排，勿主动做）

`rupost g --failed` 失败选择器（0.5–1 天）｜`--emit-env` 骨架｜T3-cURL 导入（0.5–1 天，复用 `parse_curl`）→ T3-HAR（2–4 天）｜`--format md`（与 T3 共享渲染层）｜`--interactive` 路径与模板化组合的 mock selector 单测｜body 内密码掩码（Secrets Masker 残余，header/query 已覆盖）。

### 治理与 DX 批（2026-10-27 补充）

来源：[完整开发方案 §3.2–3.3](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-27-complete-development-scheme.md)（治理批清单）与 [DX PRD](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-27-dev-workflow-dx-prd.md)（开发流程体验，痛点全部实证）。

| 卡 | 内容 | 量级 | 顺序 |
| :-- | :--- | :--- | :--- |
| **T12 文档治理批** | 横幅批：plugin 分区搁置声明+勾选修正、dev/ 同名快照归档横幅、usage_and_testing §4 对齐、QUICK_REFERENCE、sse_testing 必败命令、mock 热加载措辞、quality-hardening 头部回写、file-format-spec 两处；制度化批：状态横幅/核心集/路径保鲜 | 1–1.5 天 | P0 矛盾清零优先 |
| **DX-1 Makefile** | `make gate / gate-fast / gate-all / examples / features / coverage / prd / build / help`——一条命令门禁（本会话实证：两次被事后门禁打回，需低摩擦前置） | 0.5 天 | **可与 T10 并行**，此后每个任务的门禁都受益 |
| DX-2 流程文档修正 | development_workflow 三处漂移（不存在的 progress_summary 路径 / 手动双终端流程应指 run_all.sh / checkpoint 角色澄清）——**并入 T12 同一原子提交** | 0.25 天 | 随 T12 |
| DX-3 PRD 脚手架 | `doc/templates/prd-template.md`（预置状态横幅+事实源分工+四段+GWT）+ `make prd NAME=` | 0.5 天 | 随 DX-1 |
| DX-4 SDK 坑固化 | `make build` 检测 SDKROOT（从 progress_summary:188 的括号注释升格为代码） | 0.25 天 | 随 DX-1 |
| DX-5（P2 可选） | CI `workflow_dispatch` 手动触发 `run_all.sh` | 0.5 天 | backlog |

### 调试体验批（2026-10-28 补充）

来源：[开发者 API 调试体验 PRD](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-28-developer-debug-experience-prd.md)——D0–D7 全链路需求（R1–R10），产品侧调试体验（区别于上批的开发流程 DX）。全部需求已过三问判定；确定性项是 E2/E5 兜底基线的用户侧兄弟（AI 缺失时修复回路仍完整）。

| 卡 | 内容 | 量级 | 顺序 |
| :-- | :--- | :--- | :--- |
| **DBG-1 单请求重跑（R1）** | `rupost t api.http#3` / `--only <n\|@name>`：CLI 修复回路断裂的新立项（grep 实证无单请求选择器）；表驱动解析 + 越界零 panic + 未捕获变量走警告语义 | ~1 天 | **M1 插槽**（纯确定性，不与 AI 项竞争；MCP `run_request` Tool 的地基） |
| **DBG-2 debug-on-failure 守护（R2）** | 不可达端口 → DNS/TCP 拆分段输出、成功零噪音——测试方案 P1 既有欠账，调试链视角并入 | 0.5 天 | M1 插槽 |
| **DBG-3 trace 锚点协议（R7 补充）** | 响应头 trace-id 提取（Traceparent/X-Request-Id/X-Trace-Id → `RUPOST_FAILED_TRACE_ID`，多候选优先级 + 无值 warnings）——并入 T6 钩子卡实现 | 0.25 天 | 随 T6 |
| R3/R10（fast-follow） | `diagnose --report json` 契约测试 / TUI 流式渲染单测 | 各 0.25 天 | 随相邻改动顺带 |
| R4/R5/R8/R9 | 不在本批重复登记——E2、E5、P0、bootstrap 已在 M1/M2 既有裁决内 | — | 见 §M1/M2 |

### 云接入批（2026-10-28 补充）

来源：[能力与云接入总览 §2.3/§3.4](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-28-capability-cloud-integration-review.md)——云开发接入（四形态×五方案）与云原生介入（验证平面）的查漏结论。定位：全部为 T9 分发事务的延伸与薄壳，零新架构。

| 卡 | 内容 | 量级 | 顺序 |
| :-- | :--- | :--- | :--- |
| **CLOUD-1 容器镜像与分发壳** | Dockerfile（multi-arch、non-root）+ devcontainer feature（一行接入）+ `examples/gitops/` Argo PostSync hook 示例 manifest——解锁云原生介入形态 B/D（镜像 Job/CronJob/GitOps 部署即冒烟） | 1–2 天 | 随 T9 镜像 |
| **CLOUD-2 OSC 52 剪贴板** | TUI 内 `y` 生成 cURL 并经 ANSI OSC 52 写剪贴板（SSH/云 IDE 场景关键体验件）——吸收路线图推荐优先项正式入队 | 0.5–1 天 | P1，可与 T10 并行 |
| **CLOUD-3 无状态运行语义 + 云原生调试指引** | 文档任务：容器内默认无状态（history/cookies 不落 volume 即失，显式说明）、`~/.rupost` volume 可选挂载、port-forward 组合 + `{{base_url}}` 按服务模板化 + `--env` 对应集群上下文、终端能力探测降级（无鼠标/真彩时 TUI 简化） | 0.5 天 | 随 CLOUD-1 |
| 云原生 CRD/Operator | **gate**：需求证据（镜像形态被验证有集群内使用量 + GitOps 门禁诉求）后立项——TestKube/k6-operator 已占位不抢 | — | 远期 |
| X 护栏集群内注入 | safety 声明经环境变量注入 Job spec——随 X 系列立项时补设计 | — | 随 X 转正 |

### Won't do（当前，翻案条件见 [future_backlog](file:///Users/zsyzzx/project/rust/rupost/doc/plans/future_backlog.md) 条目 7–8）

代理抓包（T4）｜生产平台日志 adapter｜多行合并启发式（除非 T11 实验①推翻假设 1.3-2）｜gRPC/GraphQL 通用客户端（既有裁决）｜AI 帮测路线（Keploy 分界）｜**消息队列/非 HTTP 协议测试（2026-10-28 补：HTTP 验证平面定位，与 gRPC 同理由；云原生应用的 MQ 组件不覆盖）**。

---

## 三、建议执行顺序总览

```
T10 审计(0.5d) ──→ DX-1 Makefile(0.5d, 带 DX-3/4) ──→ T1 动态值(2-3d) ──→ T7 A1(2-3d) ──→ T2 链式溯源(2.5-3.5d)
T12 治理批(1-1.5d, 含 DX-2) —— 与主线并行（一个原子提交清 4 个 P0 矛盾）
        T11 探路(并行,0开发)          T3 per-entry(1-2d) ──→ T5 export ──→ T4 SmartDiff ──→ T6 钩子
                                      T8 junit(1-2d) → T9 分发
```

理由：T10 最便宜且可能是第二个泄密面；DX-1 紧随其后——此后每个任务的门禁与建档都一条命令（本会话两次被事后门禁打回的实证），先建入口后跑长任务；T12 与主线并行不阻塞；T1 是生成语义批剩余项中价值最高者（兼 ai bootstrap 地基）；T7 是全文档认定的战略最高杠杆，不被 T2 无限延后；取证链路（T3→T5→T4→T6）内部有严格依赖序；T8/T9 随时可插队（发版前必补）。
