# RuPost TUI 基础功能完善与交互细节规格说明书 (查看、调用与新建测试)

> **✅ 状态（2026-10-25）**：本规格所述三特性（Detail Tabs `h/l` 切换、侧栏 `r/R` 一键运行、
> `n` 键快速新建含 URL 智能推导）与 `E` 键外部编辑器闭环均已实现并通过冒烟测试；
> 交付记录见 `doc/plans/progress_summary.md`。原始缺口描述仅作历史留档。


本设计文档由架构师（Architect）依据 **Clean Architecture** 规范出具，旨在为 **TUI 最根本的三个核心基础功能**（查看请求与响应、一键发起用例调用、新开测试用例并闭环编辑）提供清晰的交互细节和底层代码规格设计。本版本已被冒烟测试（Smoke Test）驱动，暴露出核心状态未实现的漏洞，并为接手模型提供了确凿的解决路径。

---

## 📅 1. 核心改进目标与路线图

我们将优先完善 TUI 模式中以下三个对于测试工作流至关重要的基本功能：

1. **【查看】Response 面板的选项卡分流 (Detail Tabs)**：
   - **痛点**：目前 Response 面板只会无脑渲染 HTTP 响应结果，缺少对请求细节（请求 Header、请求 Body）及响应 Headers 的查看。
   - **冒烟测试暴露**：运行 `test_detail_tab_switching` 暴露了键盘事件尚未拦截切换 `active_detail_tab` 的问题，状态在按左右方向键后仍然停留在 `Response`。
   - **解决方案**：在右侧 Response 面板中，加入 **`Response`**、**`Headers`**、**`Request`** 三个子选项卡，由水平方向键或 `h` / `l` 键循环切换。
2. **【调用】左侧侧边栏一键直接运行 (Sidebar Direct Run)**：
   - **痛点**：目前运行测试必须先选中文件按 Enter 进入 Editor 面板，再按 `Ctrl+Enter`，无法进行极速的批量测试发包。
   - **解决方案**：在 Files 侧边栏选中非目录测试文件时，直接敲击 **`r`** 或 **`R`** 键（或 `Ctrl+Enter`），直接触发异步网络发送，结果自动在右侧回显。
3. **【新开一个测试】新建 URL 测试用例 (New URL Test Case)**：
   - **痛点**：之前缺少创建用例的功能，测试无法在 TUI 界面内部完成闭环。
   - **冒烟测试暴露**：运行 `test_smart_url_test_creation` 暴露了目前 `create_test_from_input` 仅为空白方法占位，导致期待的新建物理文件 `google_com_search.http` 等均未生成。
   - **解决方案**：在 Files 面板下，按 **`n`** 键激活底部快速输入条（Quick Input Bar），支持输入“文件名”或“URL（例如 `google.com` 或 `https://baidu.com/api`）”：
     - 若输入为 URL，自动推导文件名（如 `google_com_search.http`），并写入带有该 GET 请求的 HTTP 文件。
     - 若为普通文件名，写入默认测试模板。
     - 创建成功后更新文件树，光标自动定位到新用例，并切焦至 Editor 触发编辑状态，实现完美闭环。

---

## 🛠️ 2. 交互逻辑细节与底层技术设计

### 2.1 【查看】详情选项卡 (Detail Tabs)

由于全局键盘按键 `Tab` 已被用于 Files、Editor、Response 三个主面板焦点的轮转，为了避免快捷键冲突，Detail Tabs 的切换我们选择：
- **切换按键**：当焦点处于 `Panel::Response` 时，按 **`h`** / **`Left`** 循环向左，按 **`l`** / **`Right`** 循环向右。
- **状态字段 (`src/tui/state.rs`)**：
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
  pub enum DetailTab {
      #[default]
      Response, // 响应 Body 与状态/断言信息
      Headers,  // 请求及响应的 Headers 对照
      Request,  // 请求 Body (如果有)
  }

  pub struct AppState {
      // ... 已有字段 ...
      pub active_detail_tab: DetailTab,
  }
  ```
- **键盘事件分流 (`src/tui/handlers.rs` inside `handle_response_key`)**：
  在 `handle_response_key` 函数中，对 `KeyCode::Left` / `KeyCode::Char('h')` 和 `KeyCode::Right` / `KeyCode::Char('l')` 做出响应：
  - **`Left` / `h`**：将 `state.active_detail_tab` 依次递减：`Response` -> `Request` -> `Headers` -> `Response`。
  - **`Right` / `l`**：将 `state.active_detail_tab` 依次递增：`Response` -> `Headers` -> `Request` -> `Response`。
  - 每次切换时，必须执行 `state.response_visual_lines.clear()` 和 `state.response_scroll = 0` 以强刷预折行切片。
- **数据渲染与折行缓存 (`src/tui/ui/response.rs`)**：
  在 `rebuild_http_visual_lines` 中，判断当前的 `state.active_detail_tab`：
  - **`DetailTab::Response`**：
    保持原有绘制输出（响应状态、时延大小、断言数统计，以及 1000 行截断的 Response Body）。
  - **`DetailTab::Headers`**：
    对照输出请求和响应的 Header 映射表：
    ```rust
    raw_lines.push(Line::from(Span::styled("--- Request Headers ---", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))));
    if let Some(ref req) = state.current_request {
        for (k, v) in &req.headers {
            raw_lines.push(Line::from(format!("  {}: {}", k, v)));
        }
    }
    raw_lines.push(Line::from(""));
    raw_lines.push(Line::from(Span::styled("--- Response Headers ---", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))));
    if let Some(ref resp) = state.last_response {
        for (k, v) in &resp.headers {
            raw_lines.push(Line::from(format!("  {}: {}", k, v)));
        }
    }
    ```
  - **`DetailTab::Request`**：
    展示本次交互发包的请求 Body：
    ```rust
    raw_lines.push(Line::from(Span::styled("--- Request Body ---", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
    if let Some(ref req) = state.current_request {
        if let Some(ref body) = req.body {
            for line in body.lines() {
                raw_lines.push(Line::from(line));
            }
        } else {
            raw_lines.push(Line::from(Span::styled("  (No request body)", Style::default().fg(Color::DarkGray))));
        }
    }
    ```
- **子选项卡渲染 UI**：
  在 Response 面板顶部用 `Tabs` 绘制 `[ Response ]`、`[ Headers ]`、`[ Request ]`。活跃项使用 Cyan 高亮，不活跃项为 DarkGray。

---

### 2.2 【调用】左侧侧边栏一键直接运行

#### 2.2.1 提取公共运行调用核心 (`src/tui/handlers.rs`)
为了保持 DRY 原则，我们将原来 `handle_editor_key` 下面冗长的 `tokio::spawn` 发包逻辑提取成公用的异步方法：
`pub async fn trigger_request_execution(state: &mut AppState, event_tx: &mpsc::Sender<TuiEvent>)`
该方法包含：
1. 从内存或磁盘读取当前的 `ParsedRequest`。
2. 触发 `TuiEvent::RequestStarted` 发送。
3. 后台起 tokio 线程组分别处理流式日志的广播与网络发包计算。
4. 运行结束时向主事件循环发送 `TuiEvent::RequestFinished` 消息。

#### 2.2.2 侧边栏按键分配
在 `handle_sidebar_key` 中，如果选中非目录文件节点，且 Tab 处于 `Files` 时：
- 按 **`r`** 或 **`R`** 键（或 `Ctrl+Enter`）：
  - 自动从磁盘读取该文件内容装填到 `state.editor_text` 和 `state.editor_file_path` 中。
  - 直接调用上述 `trigger_request_execution(state, event_tx).await` 启动测试。
  - 这样用户就无需切焦到 Editor 面板也可以瞬间完成一键调用！

---

### 2.3 【新开一个测试】新建 URL 测试用例

#### 2.3.1 输入条状态扩展 (`src/tui/state.rs`)
在 `AppState` 结构体中新增控制字段：
```rust
pub struct AppState {
    // ...
    pub show_quick_input: bool,          // 是否处于输入框编辑状态
    pub quick_input_value: String,       // 快速输入的临时字符串
    pub quick_input_title: String,       // 快速输入框标题
}
```

#### 2.3.2 快速输入框的渲染布局 (`src/tui/ui/mod.rs`)
当 `state.show_quick_input == true` 时，在最底部的 Help Bar 上方绘制一个高度为 3 行的模态输入框，要求使用 HSL Yellow 边框高对比度突出显示：
```
┌── New HTTP Test File (Esc to cancel) ──────────────────────────┐
│ > https://google.com█                                          │
└────────────────────────────────────────────────────────────────┘
```
输入期间独占键盘焦点。

#### 2.3.3 按键捕获与 URL 智能转换逻辑 (`src/tui/handlers.rs` & `state.rs`)
1. 当 `state.show_quick_input == true` 时，**独占拦截所有键盘事件**：
   - **`Esc`** => 取消输入，`state.show_quick_input = false;`
   - **`Backspace`** => `state.quick_input_value.pop();`
   - **`Char(c)`** => `state.quick_input_value.push(c);`
   - **`Enter`** => 触发测试创建逻辑并退出：
     - 调用 `state.create_test_from_input()`
     - `state.show_quick_input = false;`
2. **`state.create_test_from_input()` URL 智能分析与文件名推导**：
   - 判断输入是否为 URL 链接：若以 `http://`/`https://` 开头，或包含 `.` 且不以 `.http`/`.md` 结尾（代表带有域名或端口），判定为 URL。
   - 规范前缀：若无协议，自动补齐 `https://`。
   - 智能文件名推导：解析 URL 得到 Host 与 Path，并将所有非字母数字字符替换为下划线，合并拼装文件名（如 `google.com` -> `google_com.http`，`httpbin.org/search?q=rust` -> `httpbin_org_search.http`）。
   - 模板生成：将 `GET <normalized_url>\n` 写入新文件。
   - 普通文本：直接追加 `.http` 结尾，填入 `GET http://httpbingo.org/get\n` 默认用例。
   - **定位与编辑闭环**：
     - 物理写入磁盘 -> 重新扫描目录（`filter_tui_test_files`），调用 `rebuild_visible_tree_nodes` 刷新树状视图。
     - 光标 `selected_index` 自动跳到新创建的文件行。
     - 将新内容载入 `editor_text`，并自动切焦到 `Panel::Editor`，此时提示用户可以直接按大写 **`E`** 拉起外部编辑器，构成完美的闭环。

---

### 2.4 外部编辑的挂起与恢复闭环 (`E` 键)

- 当处于非目录 Files 或 Editor 面板，按大写 **`E`** 键（Shift + e），只读历史记录拦截。
- TUI 挂起：还原原始终端（`disable_raw_mode()` 与 `LeaveAlternateScreen`）。
- 进程拉起：使用 `Command::new(editor_cmd)` 唤起由 `$VISUAL` / `$EDITOR` 指定的编辑器进程编辑目标文件。
- TUI 恢复：编辑器退出后，恢复 alternate screen 与 `enable_raw_mode()`，清屏后重新读取该文件内容热重载更新至 TUI，清空脏状态并开始运行。

---

## 🧪 3. 测试与验证保障

为了确保功能逻辑零 Bug，任何代码修改完成后必须在 `tests/tui_smoke_test.rs` 跑通以下两组用例：

1. **新建测试文件名推导与 URL 转换测试**：
   - 模拟 quick input 键入 `"https://google.com"`，断言推导文件名包含 `google_com.http`，且写入的内容包含 `"GET https://google.com"`。
   - 模拟 quick input 键入 `"httpbin.org/status/200"`，断言推导出 `httpbin_org_status_200.http`。
2. **DetailTab 状态切换测试**：
   - 模拟 Response 激活，并按 `h`/`l` 键，验证 `active_detail_tab` 在 `Response` -> `Headers` -> `Request` 之间成功流转。
