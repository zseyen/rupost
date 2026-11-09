# RuPost TUI 技术结构与架构说明 (Technical Architecture)

> **核心设计思想**：依据 **Clean Architecture (干净架构)** 原则进行系统分层，并结合 **MVU (Model-View-Update)** 单向数据流模式。将“终端 UI 的绘制和底层 Crossterm 外设交互”定位为 Frameworks & Drivers 层，将“界面状态维护和事件分发”作为 Interface Adapters 层，确保核心业务实体与测试执行用例的高内聚低耦合，构建高响应度、非阻塞的交互式命令行测试环境。

---

## 一、 架构分层设计 (Clean Architecture)

Rupost TUI 的模块组织严格遵循以下分层结构，依赖关系单向自外向内依赖：

```
       ┌─────────────────────────────────────────────────────────┐
       │             Frameworks & Drivers                        │
       │  (Ratatui 0.30, Crossterm 0.29, Tokio Async Task)       │
       └──────────────────────────┬──────────────────────────────┘
                                  │ 驱动与渲染
                                  ▼
       ┌─────────────────────────────────────────────────────────┐
       │             Interface Adapters (MVU)                    │
       │  (AppState, TuiEvent, Action, UI Renderers)             │
       └──────────────────────────┬──────────────────────────────┘
                                  │ 映射与转换
                                  ▼
       ┌─────────────────────────────────────────────────────────┐
       │             Use Cases (Application Logic)               │
       │  (TestExecutor, HistoryStorage, VariableResolver)       │
       └──────────────────────────┬──────────────────────────────┘
                                  │ 编排与调用
                                  ▼
       ┌─────────────────────────────────────────────────────────┐
       │             Entities (Core Domain)                      │
       │  (ParsedRequest, ResponseMeta, VariableContext)         │
       └─────────────────────────────────────────────────────────┘
```

### 1. Entities (核心实体层)
* **职责**：定义核心的 API 测试逻辑实体。此层完全保持独立，不引入任何 TUI 或网络库的痕迹。
* **核心结构**：
  * [ParsedRequest](file:///Users/zsyzzx/.gemini/antigravity/worktrees/rupost/design-tui-feature-spec/src/parser/mod.rs)：解析出的 HTTP 请求抽象实体。
  * [ResponseMeta](file:///Users/zsyzzx/.gemini/antigravity/worktrees/rupost/design-tui-feature-spec/src/history/model.rs)：包含状态码、Headers、以及**升级后支持 TUI 回显的 `Option<String>` 响应 Body**。
  * `VariableContext`：动态变量上下文。

### 2. Use Cases (应用逻辑层)
* **职责**：封装具体的运行用例逻辑，串联请求解析、执行和历史持久化。
* **核心组件**：
  * [TestExecutor](file:///Users/zsyzzx/.gemini/antigravity/worktrees/rupost/design-tui-feature-spec/src/runner/executor.rs)：HTTP/HTTPS 请求的发送与断言校验执行器。
  * [HistoryStorage](file:///Users/zsyzzx/.gemini/antigravity/worktrees/rupost/design-tui-feature-spec/src/history/storage.rs)：负责将测试响应追加保存至 `.rupost/history.jsonl`，并支持向前兼容的反序列化填充。

### 3. Interface Adapters (接口适配层)
* **职责**：TUI 的数据和状态转换核心。包含 [AppState](file:///Users/zsyzzx/.gemini/antigravity/worktrees/rupost/design-tui-feature-spec/src/tui/state.rs) 以及由 `app.rs` 事件循环充当的控制器。
* **数据流向**：
  * 采用经典的 **Elm/MVU (Model-View-Update)** 架构模式。
  * **Model (`AppState`)**：保存 TUI 当前的动态运行时状态，如激活面板、加载态、编辑器脏标记 `is_dirty` 等。
  * **Update (`Action` / `TuiEvent`)**：按键输入或后台响应返回被包装成统一的 `TuiEvent` 并触发状态机转移。
  * **View (`ui/mod.rs`)**：负责渲染自适应视口的 Widget 树。

### 4. Frameworks & Drivers (框架与驱动层)
* **职责**：最外层的技术基础架构与外设设备驱动。
* **技术底座**：
  * `ratatui` (0.30.2) + `crossterm` (0.29.0) + `ratatui-textarea` (0.9.2)。
  * `tokio` 异步执行协程。

---

## 二、 异步事件回路与非阻塞并发架构

TUI 系统是**单线程串行渲染**的，但 HTTP 发送和文件读取属于 IO 密集型操作。为实现高响应度且不阻塞 UI 渲染，Rupost TUI 基于 **Tokio MPSC Channel** 实现了完全解耦的并发调度架构：

```
                       ┌──────────────────────┐
                       │   crossterm::event   │ (底层按键与 Resize 事件监听)
                       │  (后台 Event 线程)    │
                       └──────────┬───────────┘
                                  │ send(TuiEvent::Input / Resize)
                                  ▼
 ┌──────────────────┐   TuiEvent   ┌───────────┐  Action   ┌──────────────────┐
 │                  ├─────────────>│           ├──────────>│                  │
 │   Tokio Runtime  │              │   MPSC    │           │     Tui App      │
 │   (Background)   │<─────────────┤  Receiver │           │   (Main Loop)    │
 │                  │  spawn task  │           │           │                  │
 └──────────────────┘              └───────────┘           └────────┬─────────┘
   (非阻塞执行请求)                                                 │
                                                                    ▼
                                                           terminal.draw() 渲染
```

### 1. 核心事件循环与异步触发
当在编辑器内触发运行请求（如 `Ctrl+R`）时，更新控制流：
1. 主线程在 `AppState` 中标记请求为加载态（`loading = true`），UI 此时保持高频（60fps）心跳渲染 Loading 效果。
2. 调度控制层使用 `tokio::spawn` 启动独立后台协程，执行异步网络请求并等待返回。
3. 后台请求完成后，向 `mpsc::Sender<TuiEvent>` 投递 `TuiEvent::RequestFinished` 消息。
4. 主线程 Event Loop 监听到该事件后，更新 `AppState::response`，将响应回显在界面上，并将 Loading 标记置为 `false`。

### 2. 状态克隆与变量继承 (Variables Cloning & Relay)
为保证链式请求在 TUI 模式中能够顺畅执行，在后台请求接收并触发 `RequestFinished` 时，UI 主循环会自动把响应解析的提取值（如捕获的登录 Token）以非阻塞的方式继承合并到全局的变量列表中：
```rust
// 当收到 RequestFinished 消息时
if let Some(resp) = &response_meta {
    // 自动克隆当前环境变量，并根据当前响应中声明的 captures 捕获新变量
    state.variables.extend(new_captured_variables);
}
```

---

## 三、 当前开发进度与实现状态 (Milestone Progress)

截至目前，Rupost TUI 的 MVP 核心功能已完整实现并全部交付：

### 1. R1. 自适应视口排版与焦点轮转 (Adaptive Layout)
- **多端响应式布局**：基于屏幕宽度与高度的自适应检测：
  - **宽屏模式 (width >= 120)**：同屏并排渲染 Files (文件列表)、Editor (编辑器)、Response (响应查看) 三栏面板。
  - **窄屏模式 (80 <= width < 120)**：双栏并排渲染 Editor 和 Response，Files 默认隐藏（可通过快捷键操作）。
  - **堆叠模式 (width < 80)**：单栏全屏渲染，顶部通过 Tab 页签切换，最大化利用小屏显示面积。
- **循环焦点控制**：支持通过 `Tab`（正向）与 `Shift+Tab`（反向）在三栏/双栏视图面板间循环轮换输入焦点。非焦点面板以 `DarkGray` 描边，当前获得焦点的活动面板外框亮显为 `Cyan` 提示。
- **极小屏幕防御**：当视口极小 (width < 40 或 height < 10) 时，强行中断布局，并全屏居中回显 `Terminal too small.`，有效防止了 Constraints 算力溢出引起的 Panic。

### 2. R2. 非阻塞网络执行与可滚动响应 (Async Run & Response Details)
- **非阻塞后台执行**：通过在内联编辑器中捕获 `Ctrl+R` 和 `Ctrl+Enter` 快捷键，异步调用 `TestExecutor`。
- **完整回显渲染**：Response 面板不仅以绿色/红色标示状态码，还完整输出耗时毫秒数、字节大小、HTTP 版本、Headers，以及美化的 JSON/文本 Response Body。
- **响应体垂直滚动**：为 Response 界面绑定了 `j/k` 键和方向键事件，允许用户在界面焦点选中 Response 栏时直接滚动浏览较长的 Response Body。

### 3. R3. 统一向前兼容的历史数据存取 (History Backward Compatibility)
- **数据源统一**：底层将 `ResponseMeta` 的 JSON 存储模型统一，加入了可选的 `body: Option<String>`，并在保存历史记录时将 body 克隆追加至 `.rupost/history.jsonl` 中。
- **向前兼容防崩**：若读取由老旧版本 Rupost 生成的、没有 `body` 字段的历史记录，反序列化器会自动将其置为 `None`，不发生任何解析 Panic，提供安全平滑的向下兼容。

### 4. R4. 未保存脏修改弹框强拦截 (Unsaved Changes Alert Modal)
- **变脏机制检测**：在 Request 文本区有修改且未执行 `Ctrl+S` 时，将 `is_dirty` 置为真。
- **防丢拦截弹窗**：在变脏状态下按 `q` 退出或切换 File 树列表时，自动触发前置悬浮确认弹窗（Modal Clear），告知 `WARNING: Unsaved changes!`。此时事件系统阻塞其余按键输入，仅当用户输入 `y` 确认放弃修改，或输入 `n/Esc` 撤回并退回编辑状态。

---

## 四、 关键技术选型与升级演进

为保持终端界面的高品质和开发的高标准，我们彻底重构并对齐了以下核心技术栈依赖：

* **`ratatui` (v0.30.2)**：全新引入的跨平台 TUI 画布。在 v0.30 中，我们全面弃用了过时的 `Frame::size`，替换为全新的 `Frame::area`。
* **`crossterm` (v0.29.0)**：负责高稳定性的多终端键盘与鼠标原始事件捕获。
* **`ratatui-textarea` (v0.9.2)**：无缝接替了原有的 `tui-textarea 0.7`（原版由于锁死 `ratatui 0.29`，与主工程新版 `ratatui 0.30` 发生了特征不匹配分裂）。完美提供带有拼写占位提示的多行内联编辑器功能。

---

## 五、 测试保障体系 (Multi-Tiered Testing Strategy)

为确保项目的高可靠性，我们建立了从单元测试到端到端集成的回归防御网：

1. **状态机逻辑验证 (TUI Smoke Tests)**：
   - 包含 [tui_smoke_test.rs](file:///Users/zsyzzx/.gemini/antigravity/worktrees/rupost/design-tui-feature-spec/tests/tui_smoke_test.rs)，用于模拟终端初始化、自适应高宽计算、脏修改拦截等纯内存状态转换。
2. **端到端集成验证 (verify_features.sh)**：
   - 全自动化测试 mock 变体拦截、网络高亮诊断 (diagnose)、变量依赖的并行 DAG 并行计算和模板初始化的防护逻辑。
3. **回归用例演示 (run_all.sh)**：
   - 运行 examples 里共计 **15 类用例集**，涵盖 WebSocket 握手、SSE 数据流落盘、Token 并行级联测试，最终结果为 **15/15 ALL PASS (全数跑通)**。
