# Original User Request

## Initial Request — 2026-07-02T15:36:27Z

为 RuPost 测试工具开发自适应的交互式终端 TUI 模式。支持测试文件加载与非阻塞执行（默认回显完整的 Header 和 Body）、统一向前兼容的响应 Body 历史数据存取管理，以及包含状态码与断言判定详情的调试结果可视化展现。

Working directory: /Users/zsyzzx/.gemini/antigravity/worktrees/rupost/design-tui-feature-spec
Integrity mode: demo

## Requirements

### R1. 自适应降级与焦点控制布局 (TUI Workspace Layout)
实现宽度与高度的自适应布局检测：
* 宽屏模式 (width >= 120)：并排渲染 Files (文件树)、Editor (编辑器)、Response (响应查看) 三栏面板。
* 窄屏模式 (80 <= width < 120)：并排渲染 Editor 和 Response 双栏面板，Files 面板默认隐藏。
* 堆叠模式 (width < 80)：单栏满屏渲染，顶部展示 Tab 标签，支持切换。
* 焦点控制：支持按 Tab 键和 Shift+Tab 键循环切换这三个面板的输入焦点，焦点所在面板的外边框高亮为 Cyan，非焦点面板为 DarkGray。
* 防御性保护：当终端尺寸极小 (width < 40 或 height < 10) 时，必须停止 Constraints 约束分割，全屏居中显示错误提示：“Terminal too small.”，防御 Panic 崩溃。

### R2. 请求执行与完整 Header/Body 响应数据渲染 (Request Run & Full Details)
加载当前高亮选择的测试文件，在编辑器中显示：
* 按下 `Ctrl+R` (或 `Ctrl+Enter`)，系统应当在后台 Tokio 异步线程中非阻塞地发出 HTTP 请求。
* 发送期间，Response 面板呈现 Loading 状态，UI 线程保持 60 帧刷新，输入与滚动不卡顿。
* 收到响应后，在 Response 面板默认展示完整的响应数据：包括响应状态码、耗时 (毫秒)、响应字节数、Headers 列表，以及 Response Body 文本。
* 响应面板需要支持垂直滚动（使用 `j`/`k` 或方向键），以便用户浏览完整的 Body 内容。

### R3. 统一且兼容的历史数据存取管理 (Compatible Unified History)
修改底层 src/history/model.rs，统一数据源：
* 将 `ResponseMeta` 的结构升级，加入可选的 `body: Option<String>`。
* 在 src/history/recorder.rs 中，保存历史记录时需将 `response.body` 克隆保存至 `ResponseMeta::body` 中并追加写入 `.rupost/history.jsonl` 中。
* 对于旧版在 JSON 中缺失 `body` 字段的历史记录，反序列化时必须默认填充为 `None`，不可发生解析 Panic 或崩溃。
* 为 `SnapshotEntry` 实现 `From<HistoryEntry>` 转换，保证旧历史导出功能的向前兼容。

### R4. 编辑器未保存强提示确认逻辑 (Unsaved Changes Alert)
在内联编辑器中：
* 如果用户修改了当前的测试文件内容，当按 `q` 退出 TUI 模式，或者在左侧 Files 面板移动光标并按回车切换加载其它测试文件时，若当前的编辑缓冲区尚未保存 (未按 `Ctrl+S` 写入物理文件)，必须强制弹出悬浮确认框（Modal Clear），提示：“Unsaved changes! Continue anyway? (y/n)”。
* 仅当用户按下 `y` 确认后，方可继续执行退出或文件切换；若按 `n` 则退回编辑状态。

### R5. 开发规范与代码解耦原则 (AGENTS.md Rule Compliance)
必须严格遵守项目根目录下的 AGENTS.md 指南：
* **解耦规范**：主入口 `main.rs` 和 `cli.rs` 仅作为适配器分发命令。禁止直接将 UI 状态生命周期或复杂渲染代码写入 `main.rs` 中，必须将核心实现隔离在库目录 `src/tui` 下，主入口仅通过 `rupost::tui::run()` 单点拉起。主程序代码行数严格限制在 150 行以内。
* **开发原则**：遵循“Think Before Coding”和“Simplicity First”原则，不写过渡设计的代码。每次新增或重构高层服务函数时，必须补充并完善对应的边缘单元测试与冒烟测试用例。

## Acceptance Criteria

### A1. 编译与冒烟单元测试通过 (Unit Tests)
- [ ] 运行 `cargo test --test tui_smoke_test` 结果为 ok，4 个单元测试（自适应计算、状态机 Action 迁移、向前兼容变量 extend）100% 通过。
- [ ] 运行 `cargo test` 检验全量单元测试与集成测试，所有已有功能 and 新写的 `test_history_entry_backward_compatibility` 均通过。
- [ ] 项目使用 `cargo build` 构建通过，且编译器无任何 Error 和 Warn。

### A2. 防御性自适应与极小尺寸验证
- [ ] 在 TUI 运行中拉伸终端，布局在三栏、双栏和堆叠单栏间切换平滑，未发生任何 Crash。
- [ ] 将终端缩放到 30x5 级别大小，界面正确显示“Terminal too small.”防御提示，且无布局溢出 Panic。

### A3. 历史数据向前兼容验证
- [ ] 旧版的无 `body` 响应历史记录（包含仅 status 和 headers 字段的 JSON 文本）可以被 `HistoryStorage` 正常读取并序列化解析，不会在读取时崩溃，且 body 反序列化结果为 `None`。
- [ ] 新历史记录被正确保存且包含了响应的完整 Body 文本。

### A4. 代码工程规范与回归测试 (Engineering & Regression Tests)
- [ ] 运行 `cargo fmt --all -- --check` 无格式差异，代码符合 Rust 格式规范。
- [ ] 运行 `cargo clippy --all-targets --all-features -- -D warnings` 全绿通过，无任何编译器警告。
- [ ] 运行 `tests/verify_features.sh` 冒烟集成测试脚本 100% 成功。
- [ ] 运行 `examples/run_all.sh` 回归测试脚本 100% 成功，保证没有造成任何旧功能（如 WebSocket, SSE）的回退。
- [ ] 完成后输出详细的变化分析与回归测试结果 Walkthrough 报告。

## Follow-up — 2026-07-02T15:39:11Z

亲爱的团队，我们已在项目工作区成功初始化了 jj 仓库（共存模式 `jj git init --colocate`）。请在接下来的 TUI MVP 和历史兼容功能开发中，务必使用 `jj` 开展分步代码 management 与原子提交（例如使用 `jj describe` 进行说明、利用 `jj new` 开启新工作修订），并且必须保证每一次分步提交均可通过全量编译和 `cargo test` 验证。


## Follow-up — 2026-07-05T09:03:47Z

对 RuPost 的长连接（SSE 和 WebSocket）默认自动落盘、5MB 日志上限与时间周期清理，以及前台 TUI 模式下基于本地日志文件的“滑动窗口/瀑布流视口”加载与查看机制进行技术合理性评估，并调研分析其它类似工具（如 HTTPie, Bruno, curl, wscat, websocat）的实现机制。

Working directory: /Users/zsyzzx/.gemini/antigravity/worktrees/rupost/design-tui-feature-spec
Integrity mode: development

## Requirements

### R1. 竞品技术机制调研 (Competitor Mechanism Research)
- 调研主流 API 客户端（如 Bruno、HTTPie、websocat 等）在处理大规模、长时间 WebSocket 收发帧和 SSE 推送流时的持久化方案与终端/UI 交互展示限制。
- 分析它们如何平衡“界面内存防卡顿”和“完整历史数据可追溯”这两个核心矛盾。

### R2. 提案方案合理性与可行性评估 (Proposed Architecture Review)
- 对 [implementation_plan.md](file:///Users/zsyzzx/.gemini/antigravity/brain/9ea58095-3a37-4fe7-b92a-044336f4cafe/implementation_plan.md) 中规划的“默认自动落盘”、“5MB 日志硬截断限额”、“7 天自动扫描清理”以及“TUI 滑动视口按需读取”的设计方案进行深度可行性与性能瓶颈评估。
- 分析在 TUI 模式下高频从本地日志文件 `seek` 和读取数据是否会造成潜在的文件 I/O 抢占、磁盘卡顿或乱序，并提出优化（如引入环形缓冲区/双向队列内存快照防抖缓存）的落地方案。

### R3. 提供具体的长连接历史查看交互原型设计 (History Viewing Interaction Design)
- 结合新拟定的 `rupost history show <target>` 交互命令，设计其具体的输入参数逻辑及针对 WebSocket 多帧的格式化终端美化渲染样式提案，确保小白用户也能顺畅操作。

## Acceptance Criteria

### 分析与报告交付 (Review Report Delivery)
- [ ] 输出一份完整的技术合理性评估与竞品调研报告（保存至 artifacts 目录下的 `technical_evaluation_report.md`）。
- [ ] 针对 TUI 的滑动视口加载算法给出伪代码或架构流程图（Mermaid）。
- [ ] 提出至少 2 点针对文件 I/O 频繁读取的防御性缓存优化策略。
