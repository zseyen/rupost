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
   - 编写并提交了完整的 [TUI 使用与测试指南](file:///doc/tui/usage_and_testing.md)，同步更新了 [README.md](file:///README.md) 并补齐了技术架构分层总结。
   - 实现了 **TUI 长连接流式实时渲染与帧控制台**。在 TUI 模式中，以 `@sse` 或者是 `@websocket` 声明的连接，均由后台 Tokio 协程驱动，UI 帧通过 `UnboundedSender` 异步通信。Response 面板自适应转为 `[WS Streaming...]` / `[SSE Streaming...]` 进行打字机式流回显和滑动窗口帧控制台，支持自动滚动追随和滚动偏移。
   - **长连接自动落盘与限额轮转机制**：所有长连接（WebSocket、SSE）运行后默认在 `.rupost/logs/` 下落盘。物理文件设置 **5MB** 安全限制防止磁盘膨胀，并在系统启动时自动清理 **7 天**前修改的陈旧日志文件。
   - **滑动窗口视口缓存机制 (Sliding Window Viewport Cache)**：前台 TUI 引入滑动视口缓存，在长连接运行期间以低开销的内存环形队列回显，在静止期/查看历史时，算法自动从物理日志文件滑动加载可见视口的前后 **N (N=50) 行** 帧记录载入 `viewport_cache` 渲染，性能大幅提升并彻底杜绝内存泄漏与 UI 卡顿。
   - **全量交互历史 show 指令检索**：在 WsRunner 优雅退出前，将全量收发帧加入响应 Body 归档至 `history.jsonl`；升级 CLI 引入 `rupost history show <target>` 指令，支持根据最近序号（如 1 代表最近一条）或 Short ID 检索回溯，并提供 `[→]` 青色发送、`[←]` 黄色接收的彩色箭头与分栏气泡气泡高亮打印。
   - **垃圾回收与清理双轨制**：实现了长连接日志的**自动异步概率懒清理**（WS/SSE 握手成功后以 1% 概率在 Tokio 后台线程池静默修剪）以及**手动 CLI 清理**（`rupost history prune` 指定天数/大小限制，`rupost history clear` 一键抹除并支持 `-y` 交互拦截和 `--all` 清空大纲数据库）。
   - **Lualine.nvim Python 虚拟环境配置**：在 `lua/configs/lualine.lua` 中新建了 Neovim 状态栏配置，优雅集成了动态环境变量检测（`VIRTUAL_ENV` / `CONDA_DEFAULT_ENV`）与 Python 黄色高亮图标组件。

---

## 下一步工作规划 (Next Steps)

- **智能 JSON Diff 引擎**：重构重放对比逻辑，对 JSON Body 默认解析并限制抖动动态字段。
- **敏感数据就地脱敏 (Secrets Masker)**：在快照落盘前，对 Authorization、Cookie 字段及 Body 敏感正则词执行就地掩码打码替换。
