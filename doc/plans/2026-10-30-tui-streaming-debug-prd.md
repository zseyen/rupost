# TUI 流式调试深化：SSE/WS 接入 + 授权 + 消息搜索（PRD）

- **状态**：Accepted（已评审，随本 PRD 同批实施）
- **日期**：2026-10-30
- **执行队列**：独立批次（`.workspace/tui-streaming`，bookmark `tui-streaming`）
- **关联文档**：`doc/tui/keybindings.md`（§搜索与过滤为本 PRD 域 3 的预留语义）、`doc/tui/usage_and_testing.md`、`doc/plans/progress_summary.md`
- **事实源分工**：本文件只承载需求与规格，不记进度——进度唯一事实源是 `doc/plans/progress_summary.md`。

---

## 一、背景与痛点（全部实证）

1. **授权链路断裂**（根因）：`src/tui/` 对 `ConfigLoader`/`EnvFileParser` 零引用，`state.variables` 初始为空 `VariableContext::new()`——TUI 中 `Authorization: Bearer {{token}}` 以字面占位符发出（未解析变量进 warnings 通道可见，但请求本身授权必失败）。CLI 标准链路在 `runner/app.rs:111-124`，可直接复用。SSE/WS 调试几乎必带 Authorization，此为"接入"的硬前提。
2. **WS 只能被动查看**：runner 执行 `@websocket` 剧本结束即 drop 连接（`ws_runner.rs:119` → `session.rs:197-201`），TUI 无法向活跃连接发送消息。可复用资产：`WsSession::send_frame`（session.rs:104）、`get_history`、指数退避重连——TUI 侧独立建会话即可，零侵入 runner。
3. **消息搜索零实现**：`keybindings.md:48-55` 早已预留 `/`、n/N、Esc 语义；`InputMode::Filter`（侧栏）是完整可克隆的模态模式；三视图（HTTP/SSE/WS）渲染均为"整列表 → scroll 切片 → Paragraph"（`ui/response.rs`），统一高亮挂载点 = 切片后、Paragraph 前。既有 bug 顺带修：`handlers/response.rs:28` 的 `response_total_lines` 计数与 wrap 后视觉行不一致。

## 二、用户故事

- **US-1 授权变量**：Given rupost.toml（或 .env）定义 `token`，When 在 TUI 运行带 `Authorization: Bearer {{token}}` 的 SSE/WS 用例，Then 请求携带解析后的真实凭据，与 CLI 同源同语义；`rupost tui --env dev --var k=v` 透传生效。
- **US-2 快速授权**：Given 未在文件中声明 Authorization，When 按 `A` 输入 `Bearer eyJ...` 并 Enter，Then 本次及后续请求自动注入该 header（显式声明优先）；状态栏可见授权已设置。
- **US-3 消息搜索**：Given 响应面板有 SSE/WS/HTTP 内容，When 按 `/` 输入子串，Then 匹配行高亮、`n`/`N` 在匹配间跳转、`Esc` 退出（query 留存可再按 n/N）；流式自动吸底在搜索态暂停。
- **US-4 WS 交互调试**：Given 编辑器为 `@websocket` 请求（含 Authorization），When 按 `W`，Then TUI 建立常驻会话并实时显示收到的帧；按 `S` 输入文本 Enter 即发送（回显为发送帧）；`D` 关闭会话；断线自动重连。
- **US-5 互斥安全**：交互会话活跃或执行中时，`r/R/W` 互拒并状态栏提示，不产生并发连接冲突。

## 三、需求清单

| # | 需求 | 验收 |
|---|---|---|
| R1 | TUI 启动加载变量上下文（toml 环境 + .env + 系统变量，顺序同 CLI） | `{{token}}` 解析单测；与 CLI 同一 `build_context` 入口 |
| R2 | `rupost tui --env/--var/--env-file` 透传 | 参数解析与分发单测 |
| R3 | `A` 快速授权模态：输入 → `__quick_auth` 变量 → 注入 Authorization（显式优先） | 注入/优先级决策表单测；模态冒烟 |
| R4 | Response 面板 `/` 搜索：三视图切片后统一高亮 + n/N 跳转 + query 留存 | 匹配/跳转纯函数决策表；TestBackend 高亮冒烟 |
| R5 | `response_total_lines` 修正为与视觉行一致 | 各视图行数一致性单测 |
| R6 | WS 交互会话：`W` 建立（复用 WsSession）→ 帧经现有 TuiEvent::WsFrame 链路回显；`S` 发送；`D` 关闭；断线重连沿用 | TcpListener WS mock 集成测试（建立/发送/回显/关闭/授权握手） |
| R7 | 互斥：会话活跃/执行中时 r/R/W 互拒 + 状态栏提示 | 状态机单测 |

## 四、非目标

- SSE 事件结构化保存（id/event/data 建 per-event 模型）——文本行搜索已覆盖需求，YAGNI
- 多 WS 会话并行、重连策略定制（沿用 session.rs 既有指数退避）
- SSE 交互发送（协议为服务端单向推送）
- runner 内部 `@websocket` 剧本执行模型的任何改动（交互会话走 TUI 独立路径）

## 五、执行计划（原子提交）

1. PRD（本文件）
2. R1/R2 变量上下文接入（app.rs 加载链 + cli.rs 透传 + 单测）
3. R3 快速授权（Auth 模态 + 注入决策表）
4. R4/R5 消息搜索（Search 模态 + 高亮 + 跳转 + 行数修正）
5. R6/R7 WS 交互会话（会话管理 + 转发 task + 发送模态 + 互斥 + 集成测试）
6. 文档收尾（keybindings/usage_and_testing/progress_summary/checkpoint/CHANGELOG）
7. 双子代理对抗审查 + 修复轮（沿用既定模式）
