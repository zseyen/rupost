# RuPost Web UI 技术方案与需求说明

> 由架构师评审流程产出（Stage 0/1）。

- **状态**：Proposed（待评审）→ Accepted（待实施）→ In Progress → Done
- **日期**：2026-11-06
- **执行队列**：独立（不并入 master execution runbook）
- **关联文档**：
  - [2026-10-25 竞争格局复审](file:///Users/zsyzzx/project/rust/rupost/doc/competition/2026-10-25-competitive-landscape-review.md)（定位约束来源）
  - [2026-10-25 测试方案 20 功能域](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-25-test-strategy-plan.md)（报告域 §5.18 的延伸）
  - [directory_structure.md](file:///Users/zsyzzx/project/rust/rupost/doc/architecture/directory_structure.md)（Clean Architecture 落位依据）
- **事实源分工**：本文件只承载需求与技术规格，不记进度——进度唯一事实源是 `doc/plans/progress_summary.md`

---

## 一、解决什么需求（背景与痛点）

### 1.0 定位前提（先回答"为什么要做"）

竞争格局复审的结论是：不做"终端 Postman"、不做平台化、TUI 赛道已红海。因此本方案的 Web UI **不是**一个网页版 API 客户端，而是执行引擎的**结果消费与协作面**：

```
构造与控制面（终端，保持一等公民）      结果消费与协作面（本方案新增）
┌─────────────────────────┐          ┌─────────────────────────┐
│ rupost t / tui / mock   │  RunEvent │ 浏览器：报告 / DAG 图 /  │
│ 编写、执行、调试、归因   │ ────────→ │ 历史 / 快照 diff / mock │
└─────────────────────────┘          │ 规则浏览与分享           │
                                     └─────────────────────────┘
```

### 1.1 痛点清单（全部实证）

| # | 痛点 | 证据 |
| :-- | :--- | :--- |
| P1 | **批量/DAG 执行结果不可分享**：跑完 `rupost t examples/batch/` 后，结果只存在于终端文本与机器 JSON 中，团队其他人（尤其实施/QA）无法浏览；跨机器分享只能截图或贴日志 | `src/reporter.rs:8` 仅有 `JsonFileReport` 机器格式，无人类可读产物 |
| P2 | **CI 门禁缺人类视图**：`t`/`replay` 以退出码 1 门禁（契约已锁定），但失败归因全靠回读终端；复审 §4 已登记补 `--report junit`，机器格式之外还缺人类可浏览的 HTML 产物 | `doc/competition/2026-10-25-competitive-landscape-review.md` §4 行 4 |
| P3 | **DAG 依赖图不可见**：`@depends-on` 拓扑排序与环检测结果只在终端日志里线性打印，用户排依赖环只能靠读文件 | `src/runner/batch.rs` 批量调度输出为终端文本 |
| P4 | **历史与快照浏览弱**：`history list` 是终端表格，快照 diff（含动态键排除）输出为终端文本；`replay --explain` 归因结果无法留档转发 | `src/history/`、`runner` 重放输出均为终端渲染 |
| P5 | **Mock 规则不可视化**：`@mock-when` 编译为 Trie 路由后，匹配逻辑对用户是黑盒，排"为什么没命中"只能读源码 | `src/mock/trie.rs` 无任何可视化出口 |

### 1.2 用户故事（Stage 0 合规）

- **测试人员**
  - Given 批量测试在 CI 跑完并产出报告，When 我把报告链接发给后端同事，Then 对方在浏览器中看到成功/失败汇总、DAG 依赖图与失败断言的请求/响应原文，无需安装 rupost。
  - Given replay 存在 3 条失败，When 我打开报告页，Then 能看到逐条 diff（含被排除的动态键标注）并直接复制归因结论。
- **用户**
  - Given 我在 Windows 上不方便装 Rust 工具链，When 同事起了一个 `rupost web` 实例，Then 我能浏览其历史记录与快照结果（只读）。
  - Given 我写了带 `@mock-when` 的 mock 配置，When 我在浏览器打开 mock 管理页，Then 能看到生效的路由表与每个分支的匹配条件。
- **开发者**
  - Given 长批量在跑，When 我打开实时页，Then SSE 推送逐用例完成事件与流式块，与 TUI 观察到的事件流一致（同一事件源）。

---

## 二、非目标（定位红线）

- **不做请求构造器全功能 UI**：不提供网页版编辑器/发送器去追 Postman/Apidog/Hoppscotch——TUI 赛道已红海（复审 §1.1），Web 侧全功能客户端是另一个赛道的红海。一句话理由：Web 的角色是消费结果，不是构造请求。
- **不引入云服务/账号/团队协作存储**：单二进制、本地优先是既有叙事（复审 B 层），不推翻。
- **不替代 TUI**：TUI 保持一等公民；Web 是增量适配层，事件层共享但状态机不共享。
- **不做用户体系**：仅本机/可信网络使用（见 4.3 安全决策）。

---

## 三、需求清单

| # | 需求 | 优先级 | 验收标准 | 兜底项 |
| :-- | :--- | :-: | :--- | :-: |
| R1 | **静态 HTML 报告**：`rupost t <file\|dir> --report html <path>`（或读既有 JSON 报告渲染 `rupost report`）产出单文件 HTML：汇总（通过率/耗时）、逐用例列表、失败断言详情（请求/响应原文、期望 vs 实际） | P0 | 对 `examples/batch/` 执行后打开 HTML 可见全部用例与失败详情；`tests/` E2E 断言 HTML 含关键字段 | 否 |
| R2 | **DAG 可视化**：报告中渲染依赖图（节点=用例，边=depends-on，环检测报错时高亮成环路径） | P1 | 含依赖环的样例文件在报告中标红成环节点 | 是（无图时退化为拓扑顺序表） |
| R3 | **`rupost web` 实时执行视图**：SSE 推送 RunEvent（用例开始/结束/流式块/汇总），页面实时刷新 | P1 | `rupost web` + 触发一次批量执行，页面无刷新逐条更新 | 是 |
| R4 | **历史与快照浏览**：`/history` 列表 + 快照 diff 只读视图（复用重放 diff 的动态键排除逻辑） | P2 | 浏览器可见最近 N 条历史与任一快照 diff | 是 |
| R5 | **Mock 规则可视化**：`/mock` 展示当前配置编译出的路由表（方法+路径模式+分支条件） | P2 | mock 配置的路由与 `@mock-when` 条件逐条可见 | 是 |
| R6 | **报告产物机器互补**：与 junit 输出（复审已登记的 backlog 项）共用 `TestReporter` 管线，互不阻塞 | P1 | junit 缺位时 HTML 报告独立可用 | — |
| R7 | **时延瀑布图诊断与网络分析**：在报告与工作台中渲染 DNS 解析、TCP 握手、TLS 协商与 TTFB 耗时瀑布图，标明证书到期告警 | P0 | 复用 `diagnose.rs`，页面原位直观可见网络各阶段耗时 | 否 |
| R8 | **.md 契约辅助编写与 cURL 导入**：向导式表单插入双角色契约块，剪贴板 cURL 实时转为 Markdown 草稿 | P1 | 页面生成符合规范的双角色代码块并支持落盘 | 是 |
| R9 | **单块原位调试发包 (In-place Runner)**：在 Web 界面对当前选中的单块发起即时执行并展示断言状态 | P1 | 页面直接呈现请求响应原文与断言对比，无须整文件运行 | 是 |

---

## 四、技术方案

### 4.1 候选路线对比与选择

| 路线 | 形态 | 优势 | 劣势 | 结论 |
| :--- | :--- | :--- | :--- | :--- |
| **A. 纯静态报告导出** | `--report html` 产出自包含单文件 HTML（内联 CSS/JS，模板渲染） | 零服务器、零新依赖面、天然 CI 产物、可直接进 artifact/邮件 | 无实时、无交互式浏览 | ✅ **Phase 0 先行**，独立解决 P1/P2/P3 |
| **B. 内嵌 Web 服务** | `rupost web` 以 axum 起 本地服务 + 嵌入式静态 SPA，SSE 推事件 | 实时视图（P3 验收）、历史/mock 浏览（R4/R5）；单二进制卖点延续（资产 `rust-embed` 编译期内嵌） | 二进制体积 +、需前端工具链（可选） | ✅ **Phase 1**，仅在 A 验证需求后投入 |
| **C. 独立守护进程 crate** | `rupost-server` 常驻 + REST API + 独立前端仓库 | 最"平台化" | 违背非目标（滑向平台竞争、破坏单二进制）、运维复杂度陡增 | ❌ 拒绝 |

**选择**：A → B 两阶段。A 是 B 的子集与验证器：模板渲染层（报告 HTML 生成）在 B 中原样复用为页面渲染后端。

### 4.2 架构落位（Clean Architecture）

```
src/web/                        ← 新增 Adapter（与 tui/、mock/server/ 同层）
├── mod.rs                      入口：rupost web 子命令装配
├── server.rs                   axum Router + SSE 端点
├── events.rs                   RunEvent 定义 + 广播通道（见 4.3 关键动作）
├── report/                     模板渲染（Phase 0 即此子目录，CLI 直调）
│   ├── mod.rs                  TestReporter → HtmlReport 渲染管线
│   └── templates/              HTML 模板（terask/template 等编译期模板，或手写 format!）
└── assets/                     rust-embed 静态资产（Phase 1）

复用（核心零改动）：
- UseCase 层：runner/executor.rs、batch.rs、history/、replay 管线原样被 Web handler 调用
- Adapter 先例：mock/server/axum_adapter.rs 证明 axum 0.8（已升级至 0.8.9 最新版）已在依赖树，HTTP 服务框架零新增，且原生支持 OpenAPI 风格的花括号路径参数
```

关键原则：**Web 是又一个 Adapter**。CLI/TUI/Web 三者消费同一批 UseCase，Web handler 内不出现任何新业务规则；出现即视为违反 Clean Architecture，退回 UseCase 层。

### 4.3 关键动作与决策点

| # | 决策 | 候选 | 推荐 + 理由 |
| :-- | :--- | :--- | :--- |
| K1 | **统一执行事件通道**（本方案唯一的内核侧改动） | (a) 扩展现有 `StreamEvent`（`src/runner/types.rs:7`，当前仅 SSE/WS 块，无 Serialize）；(b) 新增 `RunEvent` 枚举（RunStarted/CaseStarted/CaseFinished(StreamChunk)/Summary），`tokio::sync::broadcast` 广播 | **(b)**。StreamEvent 语义是"长连接协议流"，与执行生命周期是两类事件；混用会让 TUI 既有消费方受牵连。`RunEvent` 派生 `Serialize + Clone`，SSE 端点即其序列化器；TUI 侧后续可平移订阅（本期不动 TUI，Surgical） |
| K2 | 模板引擎 | (a) 手写 format!/拼接；(b) 编译期模板（tera/ructe） | **(b) tera**（或同级），单文件内联 CSS，HTML 转义交给模板引擎防注入（响应体来自外部服务，必须转义） |
| K3 | Phase 1 前端栈 | (a) 零框架 vanilla TS；(b) Svelte/Preact + Vite；(c) htmx 服务端渲染 | **(a) 先行，验证后再升级**。R3/R4/R5 交互密度低；引入 node 工具链是长期负债，等实时视图真实使用后再决策。图表（DAG）用内联 SVG 自绘（节点数有限，无需引入图库） |
| K4 | 实时通道 | (a) SSE；(b) WebSocket | **(a) SSE**。事件是服务端单向推送，与 K1 广播语义一一对应；axum 原生 `Sse` 支持；rupost 的 WS 能力是客户端向，不混淆 |
| K5 | 资产嵌入 | (a) rust-embed；(b) include_dir | **(a) rust-embed**，debug 模式回源磁盘便于开发 |
| K6 | 安全边界 | — | **默认 bind 127.0.0.1**（对齐 `mock/server/axum_adapter.rs:24` 既有姿态）；`--host` 显式开放远程；可选 `--token` Bearer；所有响应体渲染经模板转义（外部 API 响应不可信） |
| K7 | 端口 | — | 默认独立常量（避开 mock 常用端口），允许 `--port` 与 `rupost.toml` 覆盖 |

### 4.4 接口草图（Phase 1）

```
GET  /                       报告索引（最近执行）
GET  /runs/:id               单次执行报告（Phase 0 模板复用）
GET  /api/stream/:runId      SSE：RunEvent 流（执行中可订阅，结束后可回放尾帧）
GET  /api/history?limit=n    历史列表 JSON
GET  /api/history/:id/snapshot  快照 diff JSON（复用重放 diff 逻辑）
GET  /mock                   路由表只读视图（R5）
```

---

## 五、执行计划（原子提交切分）

| 阶段 | 提交切分 | 验证方式 |
| :--- | :--- | :--- |
| **Phase 0：静态报告（独立价值交付）** | ① `RunEvent` 枚举 + 广播通道 + executor 挂点（`mod tests` 单测：事件序列完整性与环检测事件）→ ② HTML 模板渲染管线 + `--report html` 接线（单测：转义/空响应/失败详情字段）→ ③ E2E：跑 `examples/batch/` 产出 HTML 并断言关键字段（`tests/`）→ ④ 文档联动（README 特性区 + guides） | 每步 `cargo test` 全绿；③ 手工冒烟截图留档 |
| **Phase 1：`rupost web` 实时服务** | ⑤ axum server 骨架 + SSE 端点消费 RunEvent（单测：SSE 帧序列）→ ⑥ 实时执行页（vanilla TS，内联 SVG DAG）→ ⑦ history/snapshot 只读路由 → ⑧ mock 路由表视图 → ⑨ E2E + README/指南更新 | E2E 用 `wiremock` 起本地服务复现真实执行；`cargo build --release` 体积增量记录入 progress_summary |
| **Phase 2（可选，按使用反馈）** | 前端栈升级决策（K3 重审）、replay diff 可视化增强 | 届时另立 PRD |

依赖关系：① 是唯一内核改动，②③④ 不依赖 Web 服务；Phase 1 全部可独立回滚（`src/web/` 目录整体移除无残留）。

---

## 六、风险与缓解

| 风险 | 缓解 |
| :--- | :--- |
| 范围蔓延为"网页版 Postman" | 非目标红线（§二）+ R 编号锁定范围；任何"编辑器/发送器"诉求显式拒绝并指向 TUI |
| 二进制体积膨胀（内嵌资产） | Phase 0 纯模板无资产；Phase 1 资产体积入验收记录；超 1MB 时触发压缩/拆分评审 |
| RunEvent 改动波及 TUI | TUI 本期不切换事件源（Surgical）；broadcast 通道新增，旧路径不动 |
| 前端工具链引入的构建复杂度 | Phase 1 锁定 vanilla TS 无 node 依赖；K3 升级需独立评审 |
| 与 junit 报告项的资源冲突 | R6 明确共用 `TestReporter` 管线，两者互为前置即可并行 |

---

## 七、评审待决问题

1. **K1 事件模型**：`RunEvent` 放 `runner/types.rs`（与 StreamEvent 同处）还是 `src/web/events.rs`？推荐前者（事件语义属执行域，Web 只是消费方）。
2. **Phase 0 的 CLI 形态**：`rupost t <file> --report html report.html` 内联产出，还是独立 `rupost report -o` 从 JSON 二次渲染？推荐**两者都要**（内联为主路径，独立子命令消费 CI 里已存在的 JSON 报告，不重跑）。
3. **Phase 1 是否随 Phase 0 同批排期**：若近期有更高优先级（如 junit），建议 Phase 0 单独交付后按使用反馈决定。
