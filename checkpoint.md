# Rupost 开发进度与状态 Checkpoint

## 当前状态 (Current State)

1. **HTTP 请求/响应基础快照录制与原样重放 (MVP 闭环)**:
   - **快照主动录制**：在 `rupost test` 命令中扩展了可选参数 `--save-snapshot <file>`，测试运行完毕后可一键将请求/响应数据序列化为结构化 JSON 快照文件。
   - **原样重放与 Host 覆写**：新增了 `rupost replay <file> [--target <url>]` 命令，加载快照并发送实际网络请求，重放时支持通过 `--target` 参数自动将请求重写到指定的内网测试网关（协议/Host/Port 覆盖），对状态码进行自动化比对和终端高亮输出。
   - **非侵入式架构**：重载 `TestResult` 增加 `request: Option<RequestSnapshot>` 数据流，并在 `execute_one` 中非侵入式地捕获源请求快照，完全保障了原本核心执行流和其它测试用例的高稳定性。
   - **自动化集成测试**：编写了专有集成用例 [tests/snapshot_replay_test.rs](file:///Users/zsyzzx/project/rust/rupost/tests/snapshot_replay_test.rs)，基于本地 Wiremock 服务器测试全过程闭环并 100% 通过。
   - **版本化提交 (JJ)**：已使用 `jj` 工具对当前所有代码修改进行安全提交，版本信息为：`feat: implement HTTP snapshot recording and replaying MVP`。

2. **声明式网络诊断与断言拓展 (Declarative Diagnostics & Assertion Metrics)**:
   - 全面支持通过添加 `# @diagnose` 注解触发网络诊断时延瀑布图和 X.509 证书深度分析。
   - 支持对 `timing.ttfb/dns/tcp/tls` 和 `cert.days_remaining` 的直接声明式契约断言，支持 `rupost diagnose --report json` 结构化导出。

3. **Sprint 5: WebSocket 协议测试与调试**:
   - 支持在 `.http` 或 `.md` 中以 `@websocket` 声明长连接，解析运行 `SEND` / `EXPECT` 等流式剧本动作，支持 MsgPack 二进制自适应解码和滑动内存历史帧环形缓冲区控制。

4. **TUI 模式的 MVP 骨架与核心事件流搭建**:
   - 成功更新 [Cargo.toml](file:///Users/zsyzzx/.gemini/antigravity/worktrees/rupost/design-tui-feature-spec/Cargo.toml) 引入 `ratatui` ("0.26")、`crossterm` ("0.27") 与 `tui-textarea` ("0.4")。
   - 在 [src/lib.rs](file:///Users/zsyzzx/.gemini/antigravity/worktrees/rupost/design-tui-feature-spec/src/lib.rs) 中完成 `pub mod tui;` 注册，实现整体编译与运行机制集成。
   - 新增并实现 [src/tui/event.rs](file:///Users/zsyzzx/.gemini/antigravity/worktrees/rupost/design-tui-feature-spec/src/tui/event.rs) 定义 `TuiEvent` 与全局行为 `Action`。
   - 新增并实现 [src/tui/state.rs](file:///Users/zsyzzx/.gemini/antigravity/worktrees/rupost/design-tui-feature-spec/src/tui/state.rs) 提供 `AppState` 状态管理、单向数据流 `update` 方法以及在请求完成时通过克隆拷贝方式安全 `extend` 变量的机制。
   - 新增并实现 [src/tui/app.rs](file:///Users/zsyzzx/.gemini/antigravity/worktrees/rupost/design-tui-feature-spec/src/tui/app.rs) 接管终端生命周期（Raw 模式，Alt Screen），并通过 background task 循环向 MPSC channel 派发 crossterm 键盘与 Resize 事件。
   - 新增并实现 [src/tui/ui/mod.rs](file:///Users/zsyzzx/.gemini/antigravity/worktrees/rupost/design-tui-feature-spec/src/tui/ui/mod.rs) 自适应视口排版引擎，支持 Wide、Narrow、Stacked 三类布局模式转换以及焦点高亮与悬浮帮助框（Help Popup）渲染。
   - 在 [tests/tui_smoke_test.rs](file:///Users/zsyzzx/.gemini/antigravity/worktrees/rupost/design-tui-feature-spec/tests/tui_smoke_test.rs) 中编写了全套自适应降级、防 Panic 单元测试，测试 100% 成功通过。

## 下一步工作规划 (Next Steps)

按照新制定的 [后续功能路线图 (Roadmap)](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-07-01-snapshot-replay-roadmap.md)，我们将推进以下扩展功能：
- **敏感数据就地脱敏 (Secrets Masker)**：在快照落盘前，对 Authorization、Cookie 字段及 Body 敏感正则词执行就地掩码打码替换。
- **智能 JSON Diff 引擎**：重构重放对比逻辑，对 JSON Body 默认解析并限制抖动动态字段。
- **打通本地测试文件执行与历史加载链路**：
  - 扫描工作区 `.http` / `.md` 测试文件并在 Files 列表展示。
  - 绑定 `Ctrl+R`（或 `Ctrl+Enter`）发送事件，在后台多线程运行 HTTP 并在 Response 栏滚动输出。
