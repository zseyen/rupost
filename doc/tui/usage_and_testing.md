# RuPost TUI 模式使用与测试指南 (Usage & Testing Guide)

本指南详细介绍了交互式终端 UI 模式 (TUI) 的运行启动、交互操作快捷键、以及三级测试防御系统的回归验证方法。

---

## 🚀 一、 使用说明 (Usage Guide)

### 1. 编译与启动
要启动 RuPost 的 TUI 模式，只需在项目根目录下通过 Cargo 运行：
```bash
cargo run -- tui
```
或直接构建 Release 二进制文件并在任意工作目录拉起：
```bash
cargo build --release
./target/release/rupost tui
```

### 2. 界面与自适应布局
TUI 会自动根据当前的终端窗口大小调整显示布局：
* **宽屏 (横向宽度 >= 120 字符)**：渲染 **Files (左侧文件树)**、**Editor (中部编辑器)** 和 **Response (右侧响应看板)** 三栏视图。
* **窄屏 (80 <= 宽度 < 120 字符)**：渲染 **Editor** 和 **Response** 两栏视图，Files 默认收起。
* **堆叠屏 (宽度 < 80 字符)**：只满屏渲染单个活动视图，顶部以 Tab 指示，支持通过 Tab 键在 `[Files]`、`[Request]`、`[Response]` 三个页签之间切换。
* **极小屏防御 (宽度 < 40 或高度 < 10)**：显示红色 `Terminal too small.` 警告屏，强力防止布局溢出崩溃。

### 3. 面板焦点与高亮 (Focus Control)
* 按 **`Tab`** 键以顺时针方向循环轮换焦点。
* 按 **`Shift+Tab`** 键以逆时针方向循环轮换焦点。
* 当前获得焦点的活动面板将高亮为 **亮青色 (Cyan) 边框**，非活动面板显示为 **深灰色 (DarkGray) 边框**。

### 4. 真正可用的核心快捷键 (Core Keybindings)
为保持简洁和极低的键盘切换开销，TUI 模式的键绑定如下：

| 按键 / 快捷键 | 作用面板 | 功能说明 |
| :--- | :--- | :--- |
| **`q`** | 全局 | 退出 TUI 模式并返回原控制终端（有未保存修改时强制弹出二次确认）。 |
| **`Tab`** | 全局 | 顺时针切换焦点面板（或者在堆叠单栏下切换标签页）。 |
| **`Shift+Tab`** | 全局 | 逆时针切换焦点面板（或者在堆叠单栏下切换标签页）。 |
| **`j` / `k` / `↑` / `↓`** | Files 面板 | 在左侧文件树列表中上下移动高亮指示光标。 |
| **`Enter`** | Files 面板 | 加载高亮选中的测试文件至编辑器中（有未保存修改时拦截）。 |
| **任意键盘字符** | Editor 面板 | 在当前 Request 缓冲区输入文本，此时编辑器将标记为变脏 `*` 状态。 |
| **`Ctrl+S`** | Editor 面板 | 保存编辑器当前缓冲区文本到物理磁盘，重置脏状态。 |
| **`Ctrl+R` 或 `Ctrl+Enter`** | Editor 面板 | 触发 Tokio 后台协程异步非阻塞执行当前 API 测试用例。 |
| **`j` / `k` / `↑` / `↓`** | Response 面板 | 对右侧响应面板的数据进行垂直滚动，方便浏览长报文 Body。 |

### 5. 脏修改拦截与模态机制
* 只要在编辑器里修改了测试内容，标题会自动变更为 `Request Editor *`，且进入脏状态。
* 此时如果按 `q` 退出或在左侧文件树里切换文件，界面正中将强制弹出红色的 `WARNING: Unsaved changes!` 模态确认浮窗：
  * 按 **`y` / `Y`**：确认丢弃当前修改，强制退出或切换。
  * 按 **`n` / `N` / `Esc`**：取消，退回编辑器，还原原输入光标位置。

---

## 🧪 二、 测试说明 (Testing Guide)

RuPost 的 TUI 模式建立了三级防御测试体系，用于在对 CLI、内核或依赖包进行修改后进行回归验证。

### 1. 单元与冒烟测试 (TUI Smoke Tests)
这套测试不涉及真实的终端 I/O 绑定，而是在虚拟 `TestBackend` 的内存上模拟运行，用于验证 TUI 的状态机转换和自适应计算：
* **核心覆盖**：
  * `test_app_state_initialization`：验证 App 初始的焦点、弹窗和布局状态。
  * `test_adaptive_layout_calculations`：验证在极小（崩溃极限）视口、宽屏、堆叠屏下的布局自适应约束计算，以及防 Panic 的边界值。
  * `test_state_transitions_via_actions`：验证 Action 状态跃迁与 Tab 焦点的正反向切换。
  * `test_unsaved_changes_confirm_modal`：验证变脏后，文件切换和退出的阻断弹窗弹出逻辑及 `Y/N` 转移状态机。
  * `test_request_finished_variable_extension`：验证异步网络数据捕获返回后，全局变量的克隆合并链路。
* **执行命令**：
  ```bash
  cargo test --test tui_smoke_test
  ```

### 2. 全量单元/集成测试回归 (Full Cargo Suite)
升级依赖库或做底层网络层改动后，需一键运行全量测试套件（包括 WebSocket 长连接和快照重放等）：
* **执行命令**：
  ```bash
  cargo test
  ```

### 3. 特性端到端集成测试 (E2E Feature Script)
通过自动编译 release 版本的二进制并拉起闭环环境下的网络诊断、诊断断言、Mock 并发等特性的自动化冒烟测试：
* **执行命令**：
  ```bash
  bash tests/verify_features.sh
  ```

### 4. 实例闭环测试 (Examples Regression)
运行本地内置的 15 种用例集，覆盖 Token 拓扑级联、流式 SSE 协议本地 Mock 大模型、WebSocket 等闭环校验：
* **执行命令**：
  ```bash
  bash examples/run_all.sh
  ```
