# RuPost TUI 技术结构与架构说明

> **设计思想**：依据 Clean Architecture 原则进行规划。将“终端 UI 的绘制和外设交互”作为 Frameworks & Drivers 层，将“界面状态维护和事件响应”作为 Interface Adapters 层，确保核心业务实体与用例逻辑不向外发生依赖，从而构建一个健壮、高响应度、可测试的 TUI 系统。

---

## 一、 架构分层设计 (Clean Architecture)

Rupost TUI 将按照以下四个层级进行构建，层级依赖关系单向自外向内：

```
       ┌─────────────────────────────────────────────────────────┐
       │             Frameworks & Drivers                        │
       │  (Ratatui, Crossterm Terminal, Tokio Async Task)        │
       └──────────────────────────┬──────────────────────────────┘
                                  │ 驱动与渲染
                                  ▼
       ┌─────────────────────────────────────────────────────────┐
       │             Interface Adapters                          │
       │  (AppState, AppController/Update, View Renderers)       │
       └──────────────────────────┬──────────────────────────────┘
                                  │ 映射与转换
                                  ▼
       ┌─────────────────────────────────────────────────────────┐
       │             Use Cases (Application Logic)               │
       │  (RequestRunner, HistoryManager, ConfigLoader)           │
       └──────────────────────────┬──────────────────────────────┘
                                  │ 编排与调用
                                  ▼
       ┌─────────────────────────────────────────────────────────┐
       │             Entities (Core Domain)                      │
       │  (ParsedRequest, Response, VariableContext)             │
       └─────────────────────────────────────────────────────────┘
```

### 1. Entities (核心实体层)
* **位置**：位于底层的核心 Domain 实体，与 UI 框架毫无关系。
* **主要对象**：
  * `ParsedRequest`：解析后的 HTTP 请求，包括方法、URL、Headers、Body。
  * `Response`：HTTP 响应体，包括状态码、Headers、响应时间、大小和 Body。
  * `VariableContext`：变量上下文环境。

### 2. Use Cases (应用逻辑层)
* **位置**：具体的应用用例，管理核心业务逻辑的数据流动。
* **主要对象**：
  * `TestExecutor`：请求执行器，负责异步发送请求、收集 Cookie、执行 Assert 校验。
  * `HistoryStorage`：历史记录的持久化读写。
  * `MarkdownFileParser` / `HttpFileParser`：解析本地请求描述文件。

### 3. Interface Adapters (接口适配层)
这是 TUI 的核心控制与适配中枢。采用 **MVU (Model-View-Update)** 即 Elm 架构：
* **Model (`AppState`)**：保存界面的数据状态（如哪个 Tab 被选中、光标的行列、响应面板是否折叠、正在加载的 loading 状态）。它将 Use Cases 层返回 of 业务数据适配为 UI 展示所需的格式。
* **Update (`AppController::update`)**：纯函数或控制器逻辑。接收 UI 动作消息（`Msg` 或 `Action`），更新 `Model`，或者派发异步副作用（例如触发一个异步的 HTTP 发送用例）。
* **View (`Renderer`)**：将 `Model` 数据渲染为 Ratatui 的具体 Widget 结构。

### 4. Frameworks & Drivers (框架与驱动层)
* **位置**：最外层，提供系统底层的具体实现。
* **主要组件**：
  * `ratatui::Terminal`：具体的绘制画布。
  * `crossterm::event`：捕获底层的标准输入按键与鼠标事件。
  * `tokio::task`：提供后台多线程运行的异步运行时，运行实际的 HTTP 发送网络 IO。

---

## 二、 异步事件流与并发架构 (Concurrency Model)

TUI 是单线程串行渲染的，但 HTTP 请求和 AI 分析必须在多线程中异步运行。为防止主线程卡顿，我们采用基于 **Tokio MPSC Channel** 的异步事件回路设计。

```
                       ┌──────────────────────┐
                       │  crossterm-event     │ (按键、尺寸缩放事件)
                       │  (后台 Event 线程)    │
                       └──────────┬───────────┘
                                  │ send(TuiEvent::Key)
                                  ▼
┌──────────────────┐   TuiEvent   ┌───────────┐  Action   ┌──────────────────┐
│                  ├─────────────>│           ├──────────>│                  │
│   Tokio Runtime  │              │   MPSC    │           │    TUI App       │
│   (Background)   │<─────────────┤  Channel  │           │  (Main Loop)     │
│                  │  spawn task  │           │           │                  │
└──────────────────┘              └───────────┘           └────────┬─────────┘
  (执行 HTTP 请求/AI)                                               │
                                                                   ▼
                                                          terminal.draw() 渲染
```

### 1. 事件通信管道 (`EventStream`)
在 TUI 启动时，创建一个 MPSC 通道：
```rust
pub enum TuiEvent {
    Input(crossterm::event::KeyEvent), // 用户输入
    Tick,                              // 定时心跳（用于动画、光标闪烁）
    Resize(u16, u16),                  // 终端窗口缩放
    RequestStarted(Uuid),              // 异步请求开始
    RequestFinished(Uuid, Result<Response, String>), // 异步请求结束
    AiStreamChunk(String),             // AI 流式分析数据块
}
```

### 2. 状态机回路设计
主循环是一个典型的 Event Loop：

```rust
// src/tui/app.rs
pub async fn run_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    mut app_state: AppState,
    mut event_rx: mpsc::Receiver<TuiEvent>,
    event_tx: mpsc::Sender<TuiEvent>,
) -> Result<()> {
    loop {
        // 1. 渲染当前状态
        terminal.draw(|f| {
            ui::render(f, &mut app_state);
        })?;

        // 2. 阻塞式接收下一个事件
        if let Some(event) = event_rx.recv().await {
            match event {
                TuiEvent::Input(key) => {
                    // 处理用户键盘输入
                    if let Some(action) = handle_key_event(key, &app_state) {
                        match action {
                            Action::Quit => break,
                            Action::SendRequest(req) => {
                                // 触发后台异步任务
                                let tx = event_tx.clone();
                                tokio::spawn(async move {
                                    tx.send(TuiEvent::RequestStarted(req.id)).await.ok();
                                    let result = execute_http_request(req).await;
                                    tx.send(TuiEvent::RequestFinished(req.id, result)).await.ok();
                                });
                            }
                            // 其他 Action 同步更新 Model
                            other => app_state.update(other),
                        }
                    }
                }
                TuiEvent::RequestStarted(id) => {
                    app_state.set_loading(id, true);
                }
                TuiEvent::RequestFinished(id, result) => {
                    app_state.set_loading(id, false);
                    match result {
                        Ok(resp) => app_state.handle_response_success(resp),
                        Err(err) => app_state.handle_response_failure(err),
                    }
                }
                TuiEvent::Resize(w, h) => {
                    // 自适应布局计算：根据新的终端高宽，动态选择 Wide/Narrow 视图排版
                    app_state.update_layout(w, h);
                }
                TuiEvent::Tick => {
                    app_state.on_tick();
                }
                _ => {}
            }
        }
    }
    Ok(())
}
```

---

## 三、 模块关系与依赖拓扑

为了体现高内聚低耦合的特质，`src/tui` 的模块之间依赖关系单向推进：

```
       ┌────────────────────────┐
       │        terminal        │ (入口驱动：控制原始模式启动、清理)
       └───────────┬────────────┘
                   │ 启动
                   ▼
       ┌────────────────────────┐
       │          app           │ (事件循环调度、协调 State 和 UI)
       └─────┬────────────┬─────┘
             │            │
             │ 更新状态    │ 渲染
             ▼            ▼
       ┌───────────┐┌───────────┐
       │   state   ││    ui     │ (自适应布局与具体面板渲染组件)
       └─────┬─────┘└─────┬─────┘
             │            │
             ▼            │
       ┌───────────┐      │
       │   event   │<─────┘ (键绑定、组件级输入拦截器)
       └───────────┘
```

### 1. `state/` 模块：无副作用的纯状态模型
负责管理所有的内部变量：
* `app_state.rs`：全局状态（当前焦点面板、浮层弹窗状态）。
* `request_state.rs`：请求编辑器内的文本缓存。
* `response_state.rs`：响应数据的滚动偏移、折叠展开树。
* `ui_state.rs`：当前终端的大小模式（Wide、Narrow、Stacked）。

### 2. `ui/` 模块：自适应布局引擎 (Adaptive Layout Engine)
基于 `Wide`、`Narrow`、`Stacked` 三种状态渲染不同的 Widget 树：
* **Wide (宽屏, width >= 120)**：划分 3 个 Rect 横向并列。
  `Layout::default().direction(Direction::Horizontal).constraints([Constraint::Percentage(25), Constraint::Percentage(40), Constraint::Percentage(35)])`
* **Narrow (窄屏, 80 <= width < 120)**：双栏布局。左侧请求，右侧响应。文件树隐藏，需通过快捷键 `Ctrl+F` 呼出浮窗。
* **Stacked (堆叠/超窄屏, width < 80)**：单栏布局。通过 Tab 键或快捷键切换主视图为 [Request]、[Response] 或 [FileTree]。

### 3. `event/` 模块：按键拦截映射
* 分级拦截：如果弹出了 Help 浮窗或 Quick Input，输入事件将被拦截在浮窗的 Input widget 中，其余快捷键失效。
* 当处于 Normal 模式，事件映射为 `Action` 后分发给控制器；处于 Edit 模式时，按键输入被送进 inline-editor 转换为字符插入事件。

---

## 四、 针对 Clean Architecture TUI 的测试路线

结合此技术结构，后续的测试开发流程应严格闭环：

1. **State 状态机逻辑测试 (100% 单元测试覆盖)**：
   在没有 Crossterm 真实参与的纯内存测试中，验证 `app_state.update(Action)` 产生的状态跃迁是否完全符合业务预期。
2. **UI 自适应布局边界值测试**：
   通过 `TestBackend` 设置极限尺寸（如 `80x24` , `120x30` , `10x10` 崩溃级尺寸），验证 UI 组件渲染时是否会 Panic（通常由于 Constraint 参数计算为负数或超限引起）。
3. **异步请求生命周期的断言测试**：
   在后台模拟发出 `RequestStarted` 事件，此时 UI 应转为 Loading 动画；接着模拟发出 `RequestFinished` 事件，Loading 动画应立即被 Response 数据板替代。整个异步流都在 `TestBackend` 的周期内模拟并测试。
