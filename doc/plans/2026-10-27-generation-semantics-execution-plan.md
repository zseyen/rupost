# RuPost 生成语义与调试取证执行计划 (2026-10-27)

> **性质**: 执行计划（ consolidation of 2026-10-25/27 竞争复审、五次对抗性子代理审查、录制需求分层裁决、生成优化方向推演）
> **事实源约定**: 本文件现为**决策推理与设计细节的记录**（§1 决策链、§1.3 已证伪假设、§4 链式导出设计长期有效）；**任务队列的活状态以 [主执行文档](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-27-master-execution-runbook.md) 为准**，本文 §3 的阶段表为落盘时快照
> **结论速览**: 云调试/日志联动方案经对抗审查降级为"钩子 + 探路"；录制四分层仅 T3 生态导入成立；主战场收敛为**生成语义批次**（凭据模板化已交付，动态值识别/Host 模板化/语义化命名/链式值溯源依次展开），A1 激活保持最高战略杠杆

---

## 1. 决策链汇总（本计划的依据）

### 1.1 起点

核心用户日常用 k9s 逐 Pod 阅读/搜索日志、多模块排障需逐个切换，提出"云开发自动调试 + 自动记录日志 + 录制"需求。

### 1.2 对抗性审查结论（7 个子代理，2026-10-27）

| 审查维度 | 裁决 | 关键发现 |
| :--- | :--- | :--- |
| 日志方案·需求 | 4/10，修改后继续 | 定位倒退风险（复活已降级的生产调试叙事）、排序倒挂、n=1、替代性门槛低（一行 shell + stern --grep） |
| 日志方案·技术 | 5/10，修复 P0 后 7/10 | **多行合并启发式空转**（CRI 对每行独立打时间戳）、`std::process::Command` 阻塞 tokio worker、多容器 Pod 需 `--all-containers`、`--log-file` 需重构 logger 初始化顺序 |
| 日志方案·商业 | 3/10，暂不投入 | stern 用户一小时可自建；"无成品占位"更可能是市场判定不值得做；与变现阶梯零对齐 |
| 录制·需求/商业 | 5/10、4/10 | T2 与 `generate` 重复；T4 撞 Reqable/Charles 重战场；闭环叙事建立在已证伪的 trace 回传假设上 |
| 录制·技术 | T2=8/10、T3=5/10、T4=3/10 | **roadmap P1-4 前提已被代码推翻**（history 已存全量 body）；replay 无 per-entry 结构化结果（钩子无挂接点）；比对仅状态码 |

### 1.3 已证伪的关键假设（防止复燃）

1. **闭环数据流错误**：trace-id 必须取自重放后的新 `TestResult.response.headers`，快照只是 replay 输入源与预期基线，不是 trace 提供者；
2. **`--timestamps` 输出中不存在"无时间戳续行"**——按行首形态做多行合并的方案在真实 kubectl 输出上空转；
3. **响应头回传 trace-id 依赖网关配置**（X-B3 系是请求传播头）——未埋点环境方案静默失效；
4. **快照路线图 P1-4 的"history 不带 Body"前提已过时**（`recorder.rs` 早已全量存 body 且带兼容层）。

### 1.4 v3 裁决（取代 v0 三切片方案）

- **唯一保留的内建项**：通用 `--on-failure` 钩子（失败时以环境变量暴露 url/status/trace-id 供任意命令消费，用户可自行接 stern 管道）；
- **T4 代理抓包**：Won't do（当前），翻案条件 = "无 GUI 环境批量捕获且拒绝装 mitmproxy"的真实需求证据；
- **生产环境平台日志 adapter**：Won't do（当前），翻案条件 = 对齐变现阶梯二的付费钩子形态；
- **排序硬约束**：A1 激活 → `--report junit` → 分发，先于一切本计划扩展项；
- **零成本探路**（编码前置）：Show HN / r/kubernetes 对比文测反响；真实集群三实验（CRI 时间戳形态 / trace 回传现状 / kubectl 组合行为）。

---

## 2. 已交付（本计划启动前）

| 提交 | 内容 |
| :--- | :--- |
| `dc1f048c` | fix(generator): generate 凭据泄露修复（复用 exporter redact 语义，红测试先行） |
| `cfad6af3` | feat(generator): 凭据模板化为 `{{变量}}`（替代 `[REDACTED]`；export 给人看 / generate 给机器跑的分工确立） |

---

## 3. 阶段规划

### Phase 0：归档债（随本计划落盘完成）

- [x] 本执行文档落盘（决策链 + 队列的单一载体）
- [x] `future_backlog.md` 登记 v3 裁决（Won't do / 推迟项 / 翻案条件）
- [x] `progress_summary.md` 登记（对抗审查 + 凭据安全批次）
- [x] `CHANGELOG.md` 登记 generate 行为变化
- [x] 快照路线图（2026-07-01）过时条目标注（P1-4 前提已推翻；P0 Secrets Masker 残余范围精确化为 body 掩码）

### Phase 1：生成语义基础批（最基本功能，先行实施）

按"最小可交付、每项可砍"拆分：

| # | 事项 | 量级 | 状态 |
| :-- | :--- | :--- | :--- |
| 1a | **语义化命名**：`### Request abc12345` → `### GET /users`、`@name get_users`（冲突追加序号） | 0.5 天 | **本轮交付** |
| 1b | **Host 模板化**：`--templatize-host` 开关，绝对 URL → `{{base_url}}/path`（命名沿用 rupost.toml 模板约定） | 1 天 | **本轮交付** |
| 1c | **动态值反向识别**：UUID/时间戳形状值 → `$uuid`/`$timestamp`（ai bootstrap 机械降级路径本体） | 2–3 天 | 待排 |
| 1d | **链式值溯源（复制型）**：下游请求值与上游响应 body/Header 精确匹配 → `@capture` 上游 + `{{}}` 下游；熵过滤 + 就近消歧 + scheme 前缀剥离；Cookie 链删头靠罐 | 2.5–3.5 天 | 待排（设计见 §4） |

### Phase 2：取证链路（服务"回归引擎"定位）

| # | 事项 | 量级 | 备注 |
| :-- | :--- | :--- | :--- |
| 2a | replay per-entry 结构化结果（`ReplayReport` 扩 `Vec<ReplayEntryResult>`） | 1–2 天 | 钩子的挂接点，承重墙 |
| 2b | Smart Diff（body/headers 比对 + 动态字段排除列表） | 2–3 天 | 依赖 2a |
| 2c | `export` 桩转实（随 2a/2b 搭车）+ 旧条目 body=None 导出语义决策 | ~20 行 | 前置决策：跳过 or 占位 |
| 2d | `--on-failure` 通用钩子 | 1–2 天 | 依赖 2a |

### Phase 3：战略主线（不被 Phase 1/2 无限挤占）

1. **A1 激活**（`rupost ai assert`，2–3 天）——竞争复审最高杠杆；完成后 1c 的机械产物可直接叠加 AI 增强；
2. `--report junit`（1–2 天，CI 接入件）；
3. 分发（cargo install / Homebrew，发版前工程事务）。

### Backlog（有真实抱怨再做）

- `rupost g --failed` 失败选择器（0.5–1 天）
- `--emit-env` 变量骨架文件（观察顶部注释是否够用）
- T3-cURL 导入（0.5–1 天，复用 `parse_curl`）→ T3-HAR（2–4 天，视 cURL 反响）
- `--format md` Markdown 输出（与 T3 共享渲染层，勿单独做）

### Won't do（当前）登记

见 [future_backlog.md §2026-10-27](file:///Users/zsyzzx/project/rust/rupost/doc/plans/future_backlog.md)：T4 代理抓包、生产环境平台日志 adapter、多行合并启发式（除非真实集群实验推翻 §1.3-2）。

---

## 4. 链式请求导出设计（Phase 1d 详细设计）

**问题本质**：历史存的是已解析快照，链式数据流（登录产出 token → 下游注入）被打平；导出 = 逆向重建数据流（值溯源）+ 正向表达（`@capture` + `{{}}`）。

**依赖分类学**：

| 类型 | 表达 |
| :--- | :--- |
| 响应 Body 产出（token 在 `$.data.access_token`） | 上游 `# @capture token from body.json.data.access_token`，下游 `Bearer {{token}}` |
| 响应 Header 产出（Location/X-Request-Id） | `@capture req_id from header.x-request-id` |
| Cookie 链（上游 Set-Cookie 会话） | **删头靠罐**：下游 Cookie 头匹配上游 Set-Cookie 时省略，依赖 AutoPersist |
| 派生型（Basic = base64(user:pass)、HMAC 签名） | 机械版不识别 → 人工变量兜底；AI 增强归 ai bootstrap |

**溯源算法**：按时间升序遍历，对下游每个高熵候选值（≥8 字符，防 `1`/`true` 误报）反序搜索上游响应（就近优先消歧；历史时间序天然无环）；scheme 前缀剥离后精确匹配。

**产物形态**：A) 单文件链（时间序即链序，`rupost t` 直接跑通）；B) 多文件 DAG（`@depends-on` + `global.` 前缀，复用 Sprint 2 全套拓扑设施）；C) curl 导出保持单命令语义，注释注明上游来源，**不做 shell 脚本链**。

**测试策略**：溯源匹配为纯函数，表驱动五决策表（单命中/多命中消歧/熵不足跳过/前缀剥离/Cookie 特判），wiremock 闭环。

---

## 5. 纪律

- 沿用 [测试方案](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-25-test-strategy-plan.md) L0–L4 纪律：功能+测试同原子提交、fmt/clippy/test 三步门禁、零外网；
- 每个阶段项独立可砍；任何与 §1.3 已证伪假设相关的翻案必须先做实验验证；
- 文档单一事实源：本文件只维护决策与队列，完成后在 progress_summary 登记，不复制状态。
