# TUI 接入 CLI 已实现能力：执行模型对齐 + 调试增强 + 快照/生成（PRD）

- **状态**：Accepted（已评审，随本 PRD 同批实施）
- **日期**：2026-11-05
- **执行队列**：独立批次（`.workspace/tui-cli-bridge`，bookmark `tui-cli-bridge`）
- **关联文档**：`doc/plans/2026-11-05-tui-streaming-debug-prd.md`（前序批次）、`doc/tui/keybindings.md`
- **事实源分工**：本文件只承载需求与规格，不记进度——进度唯一事实源是 `doc/plans/progress_summary.md`。

---

## 一、背景与痛点（勘察实证）

CLI 已实现而 TUI 缺失的能力矩阵（两轮勘察，file:line 实证）：

| 能力 | CLI | TUI 现状 | 可复用 API |
|---|---|---|---|
| 多请求文件执行 | `execute_all` 顺序级联 + @skip | **只跑 `requests.first()`** | `execute_all`（executor.rs:212） |
| 单请求选择器 | `#n`/`@name`（DBG-1） | 无 | `parse_only_selector`+`apply_selector`（selector.rs，纯函数） |
| cookie 会话延续 | 批内共享 executor | 每次触发新建 ephemeral，运行间丢失 | `export/import_cookie_state` |
| timing 四段耗时 | reporter 展示 | 不显示 | `RequestTiming`（timing.rs:10） |
| export 导出 | curl/httpie/wget + [REDACTED] | 无 | `ExporterRegistry::export_one`（plugin.rs:19） |
| 失败自动诊断 | --debug-on-failure | diagnose_report 被丢弃 | `TestResult.diagnose_report` |
| 快照录制 | --save-snapshot + --mask-secrets | 无 | `mask_snapshot_entry`+`write_snapshot_suite` |
| history→.http | generate 语义化命名 | 历史只能还原进编辑器 | `HttpGenerator::generate_with_variables` |

## 二、用户故事

- **US-1 整文件执行**：Given 编辑器为含 5 个请求的文件，When 触发执行，Then 5 个请求顺序全跑（变量级联、@skip 跳过），Results 视图逐条显示状态，响应面板跟随最新完成请求。
- **US-2 选择执行**：Given 同文件，When `#` 输入 `#3` 或 `@name`，Then 仅执行选中请求；非法选择器浮出可读错误。
- **US-3 cookie 延续**：Given 首个请求 Set-Cookie，When 再次触发任意请求，Then Cookie 头延续（不再每次重置）。
- **US-4 调试增强**：运行完成后响应面板显示四段耗时；失败自动出诊断数据（Diagnose tab 原生渲染）。
- **US-5 导出**：`X` 键弹出当前请求的 curl 命令（默认 [REDACTED] 脱敏），Shift+X 非脱敏。
- **US-6 快照与生成**：`Ctrl+S` 保存当前请求/响应快照（脱敏）；History 侧栏 `G` 一键生成 `.http` 文件入工作区。

## 三、需求清单（R1–R9）

| # | 需求 | 验收 |
|---|---|---|
| R1 | executor 新增 `execute_all_streaming`（逐请求回调），`execute_all` 委托之（行为零变化） | 既有 executor 测试全绿 + 委托等价测试 |
| R2 | TUI 多请求执行：逐请求 `TuiEvent::RequestFinished`、变量级联、`run_results` 状态 | wiremock 多请求 e2e：每请求均到达服务端 |
| R3 | Results 详情 tab：# / @name / 状态 / 耗时 / 断言 | TestBackend 渲染冒烟 |
| R4 | `#` 选择器模态 + `apply_selector` 接入 | 选择器决策表单测 |
| R5 | cookie 会话延续（cookie_bridge export/import） | 跨两次触发 Set-Cookie 延续集成测试 |
| R6 | timing 透传 + 响应面板耗时行 | 事件字段单测 + 渲染冒烟 |
| R7 | debug_on_failure 默认开 + diagnose_report 透传 + Diagnose tab（第 5 tab） | 失败请求 e2e：事件携带 report；tab 渲染冒烟 |
| R8 | `X`/`Shift+X` 导出 curl（脱敏/非脱敏弹窗） | export_one 决策表 + 弹窗冒烟 |
| R9 | `Ctrl+S` 快照保存（默认脱敏）；History `G` 生成 .http 入工作区 | 落盘内容断言（脱敏后无明文凭据）；生成文件可被解析 |

## 四、非目标

- mock server 一键启动（与流式调试弱相关，生命周期复杂）
- 多文件 batch/DAG 执行（BatchExecutor 域）
- `--cookie-file` 磁盘持久化（会话内延续即达标）
- ai assert 集成

## 五、执行计划

PRD → 域 A（R1–R5）→ 域 B（R6–R8）→ 域 C（R9）→ 文档 → 双子代理审查 + 修复 → 合并 + make gate。
