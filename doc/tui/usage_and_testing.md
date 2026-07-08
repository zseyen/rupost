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

### 6. 长连接默认自动落盘与清理 (Log Rotation & Auto Cleanup)
为保证大规模或长时间 WebSocket 帧与 SSE 流数据的完整可追溯性，且不卡顿终端渲染，RuPost 采取了双轨方案：
* **默认落盘**：所有 SSE 与 WebSocket 连接启动后，无需显示声明，底层均会默认在本地工作区的 `.rupost/logs/` 目录下生成时间戳命名的物理文件（如 `ws_20260705_1.log`），全量写入所有出站与入站数据帧。
* **文件限额**：为防止磁盘空间耗尽，单个连接的日志物理文件上限为 **5MB**（大约可存储数万帧）。文件超限后会自动截断写入并追加 `[SYSTEM] Log truncated due to size limit` 警告。
* **周期自愈与概率懒清理 (Lazy GC)**：每次 TUI/CLI 启动或发起常规请求时，**完全不进行任何文件遍历与清理**以保证 0 延时启动。只有当长连接成功建立，后台任务开始运行时，才会触发清理调用，且清理具有 **1%** 的概率（即每 100 次连接大概率只有 1 次实际执行），在 `tokio::task::spawn_blocking` 异步后台线程池中静默扫描清理 7 天前过期的日志，并对目录进行修剪（LRU 算法把超过 100MB 的旧文件删掉，降低到 80MB 以下）。

### 7. 手动垃圾清理与修剪 (Manual CLI Cleanup & Pruning)
您可以使用以下新增的 CLI 命令，显式管理和删除长连接日志以及整个历史数据库：
* **强制修剪**：运行 `rupost history prune [--days <D>] [--max-size <MB>]`
  * `--days`：指定删除多少天之前的日志文件（默认 7 天）。
  - `--max-size`：指定最大日志文件夹容量（默认 100MB），超出限制将按照文件修改时间从旧到新依次删除（LRU 算法）。
* **物理清空**：运行 `rupost history clear [-y/--yes] [--all]`
  * 默认会有红色醒目的 WARNING 确认框防止误删，提示用户输入 `y/n` 进行二次确认。
  * **`-y` / `--yes`**：自动跳过确认询问，适用于 CI/CD 或 Shell 自动化清理脚本。
  * **`--all`**：不仅清空 `logs/` 目录，还一并清空 `.rupost/history.jsonl` 主历史大纲数据库。

### 8. 历史交互回溯 (Viewing Long-Connection History)
要检索并完整阅读过往长连接或常规 HTTP 通信的详细帧日志，您可以使用 `rupost history show <target>` 交互命令：
* **列表查询**：先运行 `rupost history list` 列出历史记录的大纲，第一列为条目的 **Short ID**（例如 `8cb08160`）。
* **指定查看**：运行 `rupost history show <target>` 查看详细响应及历史长交互。其检索规则如下：
  * **按最近序号查看 (新手推荐)**：传入正整数 `N`（如 `rupost history show 1` 查看上一次刚执行完的最新日志；`show 2` 查看倒数第二条）。
  * **按 Short ID 查看**：传入短 ID 前缀（如 `rupost history show 8cb081`）。
* **彩色回显**：对 WebSocket 历史帧会采用专属的箭头标识（`[→]` 青色代表发送，`[←]` 黄色代表接收）彩色表格美化打印，SSE 推送采用绿色输出，常规 HTTP 自动格式化回显，方便用户进行离线调试。

---

## 🧪 二、 测试说明 (Testing Guide)

RuPost 的 TUI 模式建立了三级防御测试体系，用于在对 CLI、内核或依赖包进行修改后进行回归验证。

### 1. 单元与冒烟测试 (TUI Smoke Tests)
这套测试不涉及真实的终端 I/O 绑定，而是在虚拟 `TestBackend` 的内存上模拟运行，用于验证 TUI 的状态机转换和自适应计算：
* **核心覆盖**：
  * `test_app_state_initialization`：验证 App 初始的焦点、弹窗和布局状态。
  * `test_adaptive_layout_calculations`：验证在极小视口、宽屏、单栏屏下的布局自适应。
  * `test_state_transitions_via_actions`：验证 Action 状态跃迁与 Tab 焦点切换。
  * `test_unsaved_changes_confirm_modal`：验证脏文件退出阻断及弹窗转移状态机。
  * `test_request_finished_variable_extension`：验证异步数据捕获全局变量的克隆合并。
  * `test_sliding_window_viewport_loading`：验证 TUI 滚动时从物理日志文件增量滑动加载前后 N 条帧记录的算法偏移与定位精度。
  * `test_log_rotation_and_size_limit`：验证 5MB 日志硬限额在超标后截断并追加系统 SYSTEM 提示的拦截机制。
  * `test_old_logs_auto_cleanup`：验证后台异步扫描清理 7 天过期临时日志的自愈能力。
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
