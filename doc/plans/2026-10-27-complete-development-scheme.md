# RuPost 完整开发方案：分阶段需求分析与文档治理 (2026-10-27)

> **定位**: 方案的需求层与治理层——承接 [竞争复审](file:///Users/zsyzzx/project/rust/rupost/doc/competition/2026-10-25-competitive-landscape-review.md) 的战略定位（"面向实时 AI API 时代的终端回归引擎"），为 [主执行文档](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-27-master-execution-runbook.md) 队列中的每个阶段补齐"为谁解决什么问题、边界在哪"的需求分析，并对项目 104 份文档给出健康度分析与治理方案。
> **事实源分工**: 战略 = 竞争复审 ｜ 需求与治理 = 本文件 ｜ 任务队列 = 主执行文档 ｜ 进度 = [progress_summary](file:///Users/zsyzzx/project/rust/rupost/doc/plans/progress_summary.md)
> **基线**: `a8102b9e`，29 测试二进制全绿

---

## 一、需求分析总纲

### 1.1 目标用户画像（合并产品战略 §3 与生产调试 walkthrough，按本方案相关性排序）

| 画像 | 核心痛点 | 本方案服务项 |
| :--- | :--- | :--- |
| **开发者（含作者本人）** | 调试试通的请求无法无感沉淀为回归用例；沉淀产物语义失真（动态值焊死、凭据明文） | Phase 0（已交付）、Phase 1 全部 |
| **QA / 回归验证者** | 重放失败不知道哪条失败；动态字段导致假失败；失败现场难分享 | Phase 2 全部、T8 junit |
| **后端排障工程师** | API 测试失败后查日志要在 k9s 逐 Pod 翻、多模块逐个切 | Phase 2 的 `--on-failure` 钩子（委托 stern 管道），不内建聚合 |
| **CI / 平台工程** | 测试工具接入流水线摩擦大 | T8 junit、T9 分发 |
| **AI 时代的接口开发者** | 写断言仍是"写代码"而非"提需求" | T7 A1 激活 + 后续 AI 增强 |

### 1.2 需求判定准则（对抗审查沉淀的三问，所有新需求先过这三关）

1. **共性关**：是 ≥ 一个画像的共性需求，还是单一工作流（n=1 防线）？
2. **增益关**：相对一行 shell / 既有工具（stern、generate、export）的增益是否 ≥ 分钟级/次？
3. **定位关**：是否服务"终端回归引擎"定位？（排障取证须以**回归失败取证**的面目出现，不得单独发声）

### 1.3 需求四层全景

| 层 | 内容 | 载体 |
| :--- | :--- | :--- |
| 已兑现 | 17 项用户可见能力与代码一致（审计 §2.2）+ 本会话 4 提交（凭据模板化/语义命名/Host 模板化） | progress_summary |
| 待建 | Phase 1–3 队列（本文 §2 逐阶段需求分析） | 主执行文档 |
| 显式不做 | Won't do 清单（代理抓包/生产日志 adapter/gRPC/AI 帮测） | future_backlog 条目 4、7–8 |
| 远期悬空 | 27 份设想文档——本文 §2.5 给出逐域处置建议 | 文档治理批次 |

---

## 二、分阶段需求分析

### Phase 0：凭据安全与生成语义基础（已交付，需求回溯）

- **需求来源**：对抗审计发现的 P0 泄密面（`rupost g` 明文落盘凭据）+ 用户体验断点（`[REDACTED]` 占位不可运行）。
- **需求满足**：安全性（凭据零落盘）与可用性（`{{变量}}` + 四层级联即填即跑）兼得；语义命名与 Host 模板化解决"沉淀产物可读、跨环境可跑"。
- **遗留**：body 内密码字段仍明文（归 Secrets Masker 残余，backlog）；TUI 还原路径待审计（T10，P0）。

### Phase 1：生成语义收尾（T10 审计、T1 动态值识别、T2 链式值溯源）

- **用户与场景**：
  - **S1 沉淀语义正确**（开发者）：内联试通接口 → `rupost g` 沉淀 → 重跑时幂等键/时间戳应每次新鲜，而非焊死首次值——现状语义失真，属正确性缺陷而非体验缺陷。
  - **S2 链式一键沉淀**（开发者）：登录 → 业务请求的调试链，导出后应自带 `@capture` 数据流，`rupost t` 直接跑通整链——现状下游凭据模板化为**人工变量**，链式场景下是错误形态（应由上游捕获）。
  - **S3 TUI 还原安全**（开发者）：历史面板 Enter 还原进编辑器的请求若含明文凭证，是与 Phase 0 同源的第二泄密面（待 T10 确认）。
- **功能需求**：①形状识别器（UUID v4 / ISO8601 / epoch 秒与毫秒 → `$uuid`/`$timestamp`）；②值溯源引擎（下游高熵值 ↔ 上游响应 body JSONPath / 响应 Header 精确匹配 → `@capture`；就近消歧；scheme 前缀剥离）；③Cookie 链特判（匹配上游 Set-Cookie → 删头靠罐）。
- **非功能需求**：误报率（熵过滤 ≥8 字符）；产物可往返解析（每次改动跑 `HttpFileParser` roundtrip）；零外网可测（表驱动决策表 + wiremock）；生成耗时对千条历史保持线性。
- **边界（YAGNI）**：派生型依赖（Basic=base64、HMAC 签名）**不机械识别**——人工变量兜底，AI 增强归 A1 之后；不做生成时交互式询问（制造新泄密面）。

### Phase 2：取证链路（T3 per-entry、T5 export 转实、T4 Smart Diff、T6 `--on-failure`）

- **用户与场景**：
  - **S4 定点失败**（QA）：20 条快照重放 3 条失败，报告必须指出**哪三条、差在哪**——现状只有聚合计数，不可行动。
  - **S5 发布前后一致性**（QA/回归）：重放比对 body 时 `timestamp`/`request_id` 等抖动字段导致假失败——需排除列表 + `--ignore-fields`。
  - **S6 补录快照**（开发者）：忘开 `--save-snapshot` 后从历史补导（低频，搭车实现）。
  - **S7 失败自动取证**（后端排障）：测试失败瞬间自动暴露 url/status/trace-id 环境变量给任意外部命令——**这是 k9s 痛点经对抗审查降级后的最终形态**：rupost 提供"锚点数据"，聚合展示委托用户已有的 stern 管道。
- **功能需求**：`ReplayReport` 扩 `Vec<ReplayEntryResult>`；JSON diff + 默认排除列表 + `--ignore-fields`；`history export` 桩转实（**前置决策：旧条目 body=None 跳过 or 占位**）；`--on-failure <cmd>` 钩子协议。
- **非功能需求**：钩子**必须** `tokio::process::Command` + `tokio::time::timeout`（std 阻塞会占死 worker 且无超时——技术审查实证教训）；不阻塞 Fail-Fast；进入报告/历史的日志内容先经脱敏。
- **边界**：不内建 kubectl 聚合；不做 HTML 报告（backlog）；prod 环境自动取证默认关闭。

### Phase 3：战略主线（T7 A1 激活、T8 junit、T9 分发）

- **T7 A1**：需求 = "断言用自然语言提需求"（产品战略 Phase 3 的第一块真砖）；方案 A 接线已定（PRD），非功能红线：密钥仅环境变量、离线 dogfooding 闭环、`@ai_assert` 运行时指令显式裁剪（离线隔离）。**A1 兑现后 Phase 1c/1d 的机械产物直接叠加 AI 增强（派生型依赖识别是其首个验收场景）**。
- **T8 junit**：需求 = CI 平台零摩擦消费测试结果（Jenkins/GitHub）；输出契约按测试方案 §5.18 schema 纪律锁定。
- **T9 分发**：需求 = "30 秒内可安装"（cargo install → Homebrew → 二进制）；这是竞争复审 C 层唯一"不做会出局"的工程项。

### Backlog 的需求判定（为什么现在不做）

`--failed` 选择器 / `--emit-env` / cURL·HAR 导入 / `--format md` / body 掩码：均满足"增益关"但未过"共性关"（无真实抱怨样本）或应与关联项共享渲染层后一起做（`--format md` 等 T3-cURL）。触发条件各卡已注明。

### 远期悬空设想的逐域需求复核（27 份文档的处置建议）

| 设想域 | 需求复核结论 | 处置 |
| :--- | :--- | :--- |
| 插件系统 WASM（7 篇） | 与 master_plan"外部脚本"路线互相冲突；零代码零需求样本 | **搁置声明 + Won't do（当前）**；勾选造假修正 |
| AI 方案包 A/B/C（4 篇） | 实际走第四条路且已定 PRD 方案 A | 归档横幅（决策史保留） |
| 生产调试阶段 4/5/6/8 | 阶段 6（日志聚合）已被钩子方案取代；阶段 4（快照脱敏加密）关联商业阶梯二保留设想；阶段 8（WS 时序回放）留 backlog | 阶段 6 归档；4/8 backlog 一行条目 |
| 脚本引擎三胞胎（Rhai） | 零落地零抱怨 | 显式搁置声明 |
| OpenAPI 文档生成 | 商业阶梯一"OpenAPI 转可执行 Markdown"的正向能力，与 T3 导入共享渲染层 | backlog（T3 之后） |
| 契约蓝图 / 离线知识库 / AI 蒸馏 | 无需求信号 | 归档 |
| @loop / @skip-if / HTML 报告 | 老牌待办无抱怨 | backlog 维持 |

---

## 三、项目文档分析（104 份）

### 3.1 全景与健康度

> 基于审计报告（2026-10-26，98 份快照）+ 本会话增量（现 104 份）。三层沉积结构：2026-02 设想层、2026-06/07 交付高峰层、2026-10 治理层。

| 分区 | 份数 | 健康判语 |
| :--- | :-- | :--- |
| `plans/`（29） | 项目主记忆 | progress_summary 唯一事实源 + 2026-10 三件套同步良好；本会话新增执行计划/主执行文档两份（治理层延续） |
| `competition/`（7） | 权威复审 + 自降级底稿 | **全库治理标杆**；本会话新增 ain/ATAC/slumber 深剖 |
| `guides/`（10） | 健康度最高 | 与代码同步好 |
| `dev/`（15） | 化石层 | 仅 workflow/Pending_Features/github_actions 现行；同名旧 progress_summary/QUICK_REFERENCE 待横幅 |
| `plugin/ ai/ script/`（12） | **全悬空** | 零落地预研包，无搁置声明（plugin 且有勾选造假 P0） |
| `architecture/ debug/ apidoc/ tui/ cookie/ folder_execution/ assertion/`（28） | 部分现行 | 各有 1–2 份现行，其余归档/设想 |
| 根 + examples（5） | README/CHANGELOG/checkpoint 现行 | CHANGELOG 本会话已补 Unreleased 条目 |

### 3.2 矛盾清单状态更新（审计 40 处 → 本会话后）

| 状态 | 条目 |
| :--- | :--- |
| **本会话已修（5）** | ①快照路线图 P1-4 前提（勘误横幅）＋Secrets Masker 范围精确化；②AGENTS §5 CLAP 规则（P0 执行已修）；③`history show` help（P0 执行已修）；④CHANGELOG 无 Unreleased 条目（已补）；⑤generate 凭据泄露（审计时未发现的代码级 P0，`dc1f048c` 修复） |
| **待修 P0（4）→ 已清（2026-10-28 单原子提交）** | plugin feasibility 勾选造假＋分区搁置声明；dev/progress_summary 三重自相矛盾（加归档横幅）；usage_and_testing §4 编辑器宣称对齐只读现实；QUICK_REFERENCE 幽灵文件与反向结论 |
| **待修 P1（7）→ 已清（2026-10-28 第二批原子提交）** | sse_testing 必败命令（补必填 file+说明）；mock "热加载"→静态加载措辞；quality-hardening 头部回写（Phase 0–4 完成 + 基线注记）；file-format-spec @timeout 单位与 @capture 两处更正；production_debug 命令形态统一为顶层 `replay <snapshot>`（6 处）；路径漂移群 6 处（src/runner/reporter→src/reporter 等）；product_strategy 加愿景标注 |
| **待修 P2（约 12）** | 外部 worktree 失效链接、`file:///` 绝对路径、占位符、examples/README 过时等 |

### 3.3 文档治理方案

**治理批次任务卡（P1，一个原子提交 + 一个跟进提交）**：
1. **横幅批**：plugin 分区搁置声明（含路线冲突说明与勾选修正）、dev/ 两份同名快照加归档横幅、usage_and_testing §4 对齐——**已清（2026-10-28，随 P0 批次交付）**；sse_testing/mock 热加载/quality-hardening 头部/file-format-spec 已全部修正（2026-10-28 第二批）；
2. **制度化批**：①设想文档头部强制 `状态：设想中|已搁置|已归档` 横幅（效仿 competition 底稿标杆）；②核心集制度——9 份核心文档之外默认"历史"，进核心集须 progress_summary 登记；③PRD 交付物列写**模块域**而非逐文件路径（路径漂移是最高频矛盾根源）；④链接修复批（P2 顺带）。

**验收**：P0 矛盾清零；核心集 9 份全部现行且互相链接闭合；新文档进入仓库前自检"状态横幅 + 事实源分工"两项。

---

## 四、方案整合与度量

### 4.1 四层文档关系（信息流向单向，杜绝回流矛盾）

```
战略层  竞争复审（定位收敛，2026-10-25）
   ↓ 推导
需求层  本文件（画像/场景/FR/NFR/边界，2026-10-27）
   ↓ 展开
队列层  主执行文档（T1–T11 任务卡活状态）
   ↓ 执行
进度层  progress_summary（唯一事实源）+ CHANGELOG + future_backlog（取舍）
```

### 4.2 成功度量

| 阶段 | 度量 |
| :--- | :--- |
| Phase 1 | 动态值识别决策表全绿；链式导出文件 `rupost t` 直跑通；TUI 审计结论落档 |
| Phase 2 | 多请求重放可定点到条；抖动字段零假失败；钩子三平台 CI 绿；export 桩转实且有语义决策记录 |
| Phase 3 | `rupost ai assert` 离线 dogfooding 闭环；junit 被 CI 消费；`cargo install rupost` 可用 |
| 文档治理 | P0 矛盾清零；设想文档 100% 带状态横幅；新增文档 100% 声明事实源分工 |
| 全局 | 全量测试 0 失败 + fmt/clippy 双零（每提交）；progress_summary 与队列状态同步（每阶段收尾） |

### 4.3 下一个动作

1. **T10 TUI 审计**（P0，0.5 天）——主执行文档队列首项；
2. **文档治理横幅批**（P1，与 T10 同批或紧随）——一次原子提交清掉 4 个 P0 矛盾；
3. T1 动态值识别开工。
