# Rupost 开发进度与状态 Checkpoint

## 当前状态 (Current State)

1. **HTTP 请求/响应基础快照录制与原样重放 (MVP 闭环)**:
   - **快照主动录制**：在 `rupost test` 命令中扩展了可选参数 `--save-snapshot <file>`，测试运行完毕后可一键将请求/响应数据序列化为结构化 JSON 快照文件。
   - **原样重放与 Host 覆写**：新增了 `rupost replay <file> [--target <url>]` 命令，加载快照并发送实际网络请求，重放时支持通过 `--target` 参数自动将请求重写到指定的内网测试网关（协议/Host/Port 覆盖），对状态码进行自动化比对 and 终端高亮输出。
   - **非侵入式架构**：重载 `TestResult` 增加 `request: Option<RequestSnapshot>` 数据流，并在 `execute_one` 中非侵入式地捕获源请求快照，完全保障了原本核心执行流和其它测试用例的高稳定性。

2. **声明式网络诊断与断言拓展 (Declarative Diagnostics & Assertion Metrics)**:
   - 全面支持通过添加 `# @diagnose` 注解触发网络诊断时延瀑布图和 X.509 证书深度分析。
   - 支持对 `timing.ttfb/dns/tcp/tls` and `cert.days_remaining` 的直接声明式契约断言，支持 `rupost diagnose --report json` 结构化导出。

3. **Sprint 5: WebSocket 协议测试与调试**:
   - 支持在 `.http` 或 `.md` 中以 `@websocket` 声明长连接，解析运行 `SEND` / `EXPECT` 等流式剧本动作，支持 MsgPack 二进制自适应解码和滑动内存历史帧环形缓冲区控制。

4. **TUI 模式功能完善与安全拦截闭环**:
   - **自适应视口排版**：支持 Wide、Narrow、Stacked 三类布局模式转换，并在超小尺寸 (width < 40 或 height < 10) 时强制全屏回显防御性警告 `Terminal too small.`，避免 Constraints 分割溢出 Panic。
   - **非阻塞异步发送请求**：在内联编辑器状态下，按 `Ctrl+R` 或 `Ctrl+Enter` 实时解析当前 buffer 文本，在后台 Tokio 协程中拉起 `TestExecutor` 发送网络请求，并实时捕获回传变量、状态码和断言信息。
   - **Response 数据回显**：接收到响应后，自动刷新 UI 并在右侧栏完整展示状态码、耗时、大小、Headers 以及完整的 Response Body 文本，支持使用 `j/k` 进行滚动浏览。
   - **未保存修改强拦截弹窗**：在编辑器已变脏的情况下，若按 `q` 退出或切换文件列表，强力弹出悬浮红色确认框，告知 `WARNING: Unsaved Changes!`，阻断其它按键输入，按 `y` 确认，按 `n/Esc` 撤回并还原光标。
   - **向前兼容的历史数据源**：统一底层 `HistoryEntry` 与 `ResponseMeta` 的 Body 序列化机制。对于遗留的无 `body` 字段的历史数据，反序列化时能默认填充 `None`，不发生任何解析 Panic，完美满足向下兼容路线。

5. **回归测试与格式化保证**:
   - 补充了未保存拦截状态转移的单元测试用例。
   - `cargo test` 全量测试套件 100% 成功通过 (all passed)。
   - `cargo fmt` 与 `cargo clippy -- -D warnings` 在最新依赖下全绿通过，消除了所有多版本 Widgets 冲突、manual prefixes 截断与 collapsible nested ifs 的警告。
   - 成功将 TUI 核心依赖升级至最新稳定版：`ratatui 0.30`、`crossterm 0.29` 与 `ratatui-textarea 0.9`，并彻底解决了两代 `ratatui` 特征不匹配分裂的难题。
   - 运行冒烟回归脚本 `tests/verify_features.sh` 与示例脚本 `examples/run_all.sh` 均成功通过，所有内置示例 **15/15 成功通过 (ALL PASS)**。

---

## 下一步工作规划 (Next Steps)

- **敏感数据就地脱敏 (Secrets Masker)**：在快照落盘前，对 Authorization、Cookie 字段及 Body 敏感正则词执行就地掩码打码替换。
- **智能 JSON Diff 引擎**：重构重放对比逻辑，对 JSON Body 默认解析并限制抖动动态字段。
- **TUI 模式的 WebSocket/SSE 协议扩展支持**：
  - 在 TUI 中对以 `@websocket` 或 `@sse` 声明的连接，支持触发后台长连接拉起并在终端以流式动态回显调试帧信息。
