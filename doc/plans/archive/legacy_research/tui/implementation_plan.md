# RuPost TUI 实施方案

> 基于方案三（混合自适应布局）的详细实施计划

---

## 📋 项目概览

**目标**：为 RuPost 打造现代化、高效的 TUI 界面，提供媲美 GUI 的开发体验

**核心特性**：
- ✅ 自适应布局（支持宽屏、窄屏、超窄屏）
- ✅ 快速创建请求（直接输入 URL、快速编辑）
- ✅ 文件管理（浏览、创建、编辑 .http 文件）
- ✅ 实时响应展示（格式化、高亮、折叠）
- ✅ 历史记录管理
- ✅ 键盘优先交互

---

## 🎯 核心功能设计

### 1. 快速请求创建（Quick Request）

#### 功能描述
用户可以在 TUI 界面中快速输入并发送 HTTP 请求，无需创建文件。

#### 交互流程

**方式一：快捷命令（推荐）**

```
按键：Ctrl+N（New Quick Request）

┌─────────────────────────────────────────────────────────────┐
│ 快速请求                                     Esc 取消       │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  输入 URL 或完整请求：                                       │
│  ┌─────────────────────────────────────────────────────┐   │
│  │ https://api.github.com/users/octocat               │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
│  💡 提示：                                                   │
│  • 直接输入 URL → 自动识别为 GET 请求                       │
│  • 输入 "POST https://..." → 识别为 POST                    │
│  • 输入 "curl ..." → 自动解析 curl 命令                     │
│                                                             │
│  [Tab] 展开详细编辑器  [Enter] 直接发送  [Esc] 取消         │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

**方式二：智能解析**

用户输入自动解析：
- `https://api.example.com/users` → `GET https://api.example.com/users`
- `POST https://api.example.com/users` → `POST https://api.example.com/users`
- `curl -X POST https://...` → 完整解析 curl 命令

**方式三：展开为完整编辑器（按 Tab）**

```
┌─────────────────────────────────────────────────────────────┐
│ 快速请求编辑器                              Ctrl+S 发送     │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Method: [GET ▼]  URL: https://api.github.com/users/octocat│
│                                                             │
│  ┌─ Headers ───────────────────────────────────────────┐   │
│  │ Authorization: Bearer {{token}}          [+ Add]    │   │
│  │ Accept: application/json                            │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
│  ┌─ Body ──────────────────────────────────────────────┐   │
│  │ (empty)                                             │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
│  ┌─ Assertions ────────────────────────────────────────┐   │
│  │ @assert status == 200                    [+ Add]    │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
│  ☑ 保存到文件： _quick_requests/2026-02-03_001.http        │
│                                                             │
│  [Ctrl+Enter] 发送  [Ctrl+S] 保存并发送  [Esc] 取消         │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

#### 实现要点

```rust
// src/tui/quick_request.rs

pub struct QuickRequestInput {
    input: String,
    cursor_pos: usize,
    mode: InputMode,  // Simple, Expanded
}

impl QuickRequestInput {
    /// 解析用户输入
    pub fn parse(&self) -> Result<Request> {
        // 1. 尝试解析为 curl 命令
        if self.input.starts_with("curl") {
            return parse_curl_command(&self.input);
        }
        
        // 2. 尝试解析为 method + url
        if let Some(parsed) = parse_method_url(&self.input) {
            return Ok(parsed);
        }
        
        // 3. 默认为 GET 请求
        Ok(Request {
            method: Method::GET,
            url: self.input.clone(),
            headers: HashMap::new(),
            body: None,
        })
    }
    
    /// 保存到临时文件
    pub fn save_to_file(&self, dir: &Path) -> Result<PathBuf> {
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let filename = format!("quick_{}.http", timestamp);
        let path = dir.join(filename);
        
        let content = self.to_http_format();
        fs::write(&path, content)?;
        
        Ok(path)
    }
}
```

---

### 2. 文件创建与编辑

#### 功能描述
在 TUI 中直接创建和编辑 `.http` 文件，无需切换到外部编辑器。

#### 交互流程

**创建新文件（Ctrl+T）**

```
┌─────────────────────────────────────────────────────────────┐
│ 创建测试文件                                                │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  文件名：                                                    │
│  ┌─────────────────────────────────────────────────────┐   │
│  │ tests/user_api.http                                 │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
│  模板：                                                      │
│  ○ 空白文件                                                  │
│  ● 基础 HTTP 请求模板                                        │
│  ○ 带断言的测试模板                                          │
│  ○ 从当前请求创建                                            │
│                                                             │
│  [Enter] 创建  [Esc] 取消                                   │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

**内置编辑器（Ctrl+E）**

```
┌─────────────────────────────────────────────────────────────┐
│ 编辑: tests/user_api.http              [保存] [关闭] [帮助] │
├─────────────────────────────────────────────────────────────┤
│  1  ### Get User Info                                       │
│  2  @name = GetUser                                         │
│  3  GET {{base_url}}/users/1                                │
│  4  Authorization: Bearer {{token}}                         │
│  5                                                          │
│  6  @assert status == 200                                   │
│  7  @assert body.id == 1                                    │
│  8                                                          │
│  9  ###                                                     │
│ 10  │                                                       │
│     └─ [INSERT MODE]                                        │
│                                                             │
│  💡 智能提示：                                               │
│  • 输入 @ → 显示元数据指令补全                              │
│  • 输入 {{ → 显示变量补全                                   │
│  • 输入 GET/POST → 自动格式化                               │
│                                                             │
├─────────────────────────────────────────────────────────────┤
│ [i] 编辑 [Esc] 退出编辑 [Ctrl+S] 保存 [Ctrl+R] 运行当前块   │
└─────────────────────────────────────────────────────────────┘
```

#### 实现要点

```rust
// src/tui/editor.rs

pub struct InlineEditor {
    content: String,
    cursor: Cursor,
    mode: EditorMode,  // Normal, Insert, Command
    syntax_highlighter: HttpSyntaxHighlighter,
    autocomplete: AutoComplete,
}

impl InlineEditor {
    /// 智能补全
    pub fn get_completions(&self) -> Vec<Completion> {
        let line = self.current_line();
        let cursor = self.cursor.column;
        
        // 1. 检测元数据指令
        if line.trim_start().starts_with('@') {
            return vec![
                Completion::new("@name", "Request name"),
                Completion::new("@timeout", "Timeout in ms"),
                Completion::new("@assert", "Assertion"),
                Completion::new("@capture", "Capture variable"),
                Completion::new("@skip", "Skip this request"),
            ];
        }
        
        // 2. 检测变量
        if self.is_in_variable_context(cursor) {
            return self.get_available_variables();
        }
        
        // 3. 检测 HTTP 方法
        if self.is_request_line() {
            return vec![
                Completion::new("GET", ""),
                Completion::new("POST", ""),
                Completion::new("PUT", ""),
                Completion::new("DELETE", ""),
                Completion::new("PATCH", ""),
            ];
        }
        
        vec![]
    }
    
    /// 语法高亮
    pub fn highlight(&self) -> Vec<Span> {
        self.syntax_highlighter.highlight(&self.content)
    }
}

/// 模板系统
pub struct FileTemplate;

impl FileTemplate {
    pub fn basic_request() -> &'static str {
        r#"### Request Name
GET https://api.example.com/endpoint
Content-Type: application/json

{
  "key": "value"
}
"#
    }
    
    pub fn with_assertions() -> &'static str {
        r#"### Test Request
@name = TestRequest
@timeout = 3000

GET {{base_url}}/api/endpoint
Authorization: Bearer {{token}}

@assert status == 200
@assert body.success == true
@capture result_id = body.id
"#
    }
}
```

---

### 3. 主界面布局

#### 默认布局（宽屏）

```
┌─────────────────────────────────────────────────────────────────────────────┐
│ RuPost TUI v1.0                [dev]                      ⚡ Ready  [?] Help │
├──────────────────────┬──────────────────────────────┬───────────────────────┤
│                      │                              │                       │
│  📝 Request          │  📊 Response                 │  📂 Files             │
│  ─────────────────   │  ─────────────────────────── │  ───────────────────  │
│                      │                              │                       │
│  GET /users/1        │  Status: 200 OK              │  ▼ tests/             │
│                      │  Time: 145ms                 │    ● user_api.http    │
│  Headers:            │  Size: 1.2KB                 │      auth.md          │
│  Auth: Bearer {{t}}  │                              │  ▼ examples/          │
│                      │  [Headers] [Body] [Assert]   │    basic.http         │
│  Body:               │                              │  ▶ _quick_requests/   │
│  (empty)             │  {                           │                       │
│                      │    "id": 1,                  │  ─────────────────    │
│  Assertions:         │    "name": "Alice"           │  📜 History (10)      │
│  @assert stat==200   │  }                           │  ───────────────────  │
│                      │                              │  GET /users/1  ✓ 200  │
│ [Send] Ctrl+Enter    │  Assertions: 1/1 ✓           │  POST /login   ✓ 201  │
│                      │                              │                       │
├──────────────────────┴──────────────────────────────┴───────────────────────┤
│ Ctrl+N 新建请求 | Ctrl+T 新建文件 | Ctrl+E 编辑 | Ctrl+F 搜索 | q 退出     │
└─────────────────────────────────────────────────────────────────────────────┘
```

#### 窄屏优化（<120 列）

自动切换为垂直堆叠布局：

```
┌───────────────────────────────────────────────┐
│ RuPost [dev]                [?] q             │
├───────────────────────────────────────────────┤
│ 📝 Request                                    │
│ GET /users/1                                  │
│ Headers: Auth: Bearer {{token}}               │
│ [Send] Ctrl+Enter                             │
├───────────────────────────────────────────────┤
│ 📊 Response           ✓ 200 OK 145ms          │
│ { "id": 1, "name": "Alice" }                  │
│ Assertions: 1/1 ✓                             │
├───────────────────────────────────────────────┤
│ 📂 Files                                      │
│ ● user_api.http  [History] [Quick]           │
├───────────────────────────────────────────────┤
│ Ctrl+N 新建 | Ctrl+T 文件 | Tab 切换         │
└───────────────────────────────────────────────┘
```

---

## 🏗️ 技术架构

### 目录结构

```
src/tui/
├── mod.rs                  # 模块导出
├── app.rs                  # 应用主状态机
├── terminal.rs             # 终端初始化与清理
│
├── ui/                     # UI 层
│   ├── mod.rs
│   ├── layout.rs           # 自适应布局引擎
│   ├── renderer.rs         # 渲染器
│   └── components/         # UI 组件
│       ├── request_panel.rs
│       ├── response_panel.rs
│       ├── file_tree.rs
│       ├── quick_input.rs
│       ├── editor.rs
│       ├── status_bar.rs
│       └── help_popup.rs
│
├── state/                  # 状态管理
│   ├── mod.rs
│   ├── app_state.rs        # 全局状态
│   ├── request_state.rs    # 请求状态
│   └── ui_state.rs         # UI 状态
│
├── event/                  # 事件处理
│   ├── mod.rs
│   ├── handler.rs          # 事件处理器
│   ├── key_bindings.rs     # 按键映射
│   └── input.rs            # 输入处理
│
├── utils/                  # 工具函数
│   ├── syntax.rs           # 语法高亮
│   ├── autocomplete.rs     # 自动补全
│   └── formatter.rs        # 格式化
│
└── theme/                  # 主题系统
    ├── mod.rs
    ├── colors.rs           # 颜色定义
    └── styles.rs           # 样式定义
```

### 核心数据流（Elm Architecture）

```
┌────────────────────────────────────────────────────────┐
│                                                        │
│                    User Input                          │
│                        │                               │
│                        ▼                               │
│                ┌───────────────┐                       │
│                │ Event Handler │                       │
│                └───────┬───────┘                       │
│                        │                               │
│                        ▼                               │
│                ┌───────────────┐                       │
│                │  Update State │                       │
│                └───────┬───────┘                       │
│                        │                               │
│                        ▼                               │
│                ┌───────────────┐                       │
│                │  Render View  │                       │
│                └───────┬───────┘                       │
│                        │                               │
│                        ▼                               │
│                   Terminal UI                          │
│                                                        │
└────────────────────────────────────────────────────────┘
```

### 状态定义

```rust
// src/tui/state/app_state.rs

pub struct AppState {
    // UI 状态
    pub ui: UiState,
    
    // 当前请求
    pub current_request: Option<Request>,
    
    // 最后响应
    pub last_response: Option<Response>,
    
    // 文件树
    pub file_tree: FileTree,
    
    // 历史记录
    pub history: Vec<HistoryEntry>,
    
    // 变量上下文
    pub variables: VariableContext,
    
    // 快速输入
    pub quick_input: Option<QuickRequestInput>,
    
    // 编辑器
    pub editor: Option<InlineEditor>,
}

pub struct UiState {
    pub active_panel: Panel,
    pub layout_mode: LayoutMode,  // Wide, Narrow, Mobile
    pub show_help: bool,
    pub show_settings: bool,
}

pub enum Panel {
    Request,
    Response,
    FileTree,
    QuickInput,
    Editor,
}

pub enum LayoutMode {
    Wide,      // 三栏
    Narrow,    // 两栏
    Stacked,   // 垂直堆叠
}
```

---

## 🎹 按键绑定

### 全局快捷键

| 按键 | 功能 | 说明 |
|------|------|------|
| `?` | 显示帮助 | 显示所有快捷键 |
| `q` | 退出应用 | 需确认 |
| `Ctrl+C` | 强制退出 | 无确认 |
| `Ctrl+N` | 快速请求 | 打开快速输入框 |
| `Ctrl+T` | 新建文件 | 创建 .http 文件 |
| `Ctrl+E` | 编辑文件 | 打开编辑器 |
| `Ctrl+F` | 搜索 | 全局搜索 |
| `Ctrl+R` | 运行请求 | 发送当前请求 |
| `Ctrl+H` | 历史记录 | 查看历史 |
| `Tab` | 切换面板 | 顺序切换 |
| `Shift+Tab` | 反向切换 | 逆序切换 |

### 面板内快捷键

**请求面板**
- `i` - 进入编辑模式
- `Esc` - 退出编辑模式
- `Ctrl+Enter` - 发送请求

**响应面板**
- `/` - 搜索内容
- `n` / `N` - 下一个/上一个匹配
- `j` / `k` - 滚动
- `Enter` - 展开/折叠 JSON 节点

**文件树**
- `j` / `k` - 上下移动
- `h` / `l` - 折叠/展开
- `Enter` - 打开文件
- `d` - 删除文件（需确认）
- `r` - 重命名

### Vim 风格导航（可选）

- `h` `j` `k` `l` - 方向移动
- `gg` - 跳到顶部
- `G` - 跳到底部
- `Ctrl+D` - 向下翻半页
- `Ctrl+U` - 向上翻半页

---

## 🎨 UI 组件设计

### 1. Quick Input 组件

```rust
// src/tui/ui/components/quick_input.rs

use ratatui::{
    widgets::{Block, Borders, Paragraph},
    layout::Alignment,
    style::{Color, Modifier, Style},
};

pub struct QuickInput<'a> {
    input: &'a str,
    cursor_pos: usize,
    placeholder: &'a str,
}

impl<'a> Widget for QuickInput<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .title("快速请求")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));
        
        let inner = block.inner(area);
        block.render(area, buf);
        
        // 渲染输入框
        let input_text = if self.input.is_empty() {
            Span::styled(
                self.placeholder,
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC)
            )
        } else {
            Span::raw(self.input)
        };
        
        let paragraph = Paragraph::new(input_text)
            .alignment(Alignment::Left);
        paragraph.render(inner, buf);
        
        // 渲染光标
        if self.cursor_pos < area.width as usize {
            buf.get_mut(inner.x + self.cursor_pos as u16, inner.y)
                .set_style(Style::default().bg(Color::White).fg(Color::Black));
        }
    }
}
```

### 2. Syntax Highlighter

```rust
// src/tui/utils/syntax.rs

pub struct HttpSyntaxHighlighter;

impl HttpSyntaxHighlighter {
    pub fn highlight(&self, content: &str) -> Vec<Span> {
        let mut spans = Vec::new();
        
        for line in content.lines() {
            // HTTP 方法
            if let Some(method) = self.extract_method(line) {
                spans.push(Span::styled(
                    method,
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                ));
            }
            
            // 元数据指令
            if line.trim_start().starts_with('@') {
                spans.push(Span::styled(
                    line,
                    Style::default().fg(Color::Yellow)
                ));
            }
            
            // 变量
            for var in self.extract_variables(line) {
                spans.push(Span::styled(
                    var,
                    Style::default().fg(Color::Magenta)
                ));
            }
        }
        
        spans
    }
}
```

---

## 📅 实施路线图

### Phase 1: MVP（2-3 周）

**目标**：可用的基础 TUI 界面

- [x] 项目结构搭建
- [ ] 基础框架集成（Ratatui + Crossterm）
- [ ] 三栏布局实现
- [ ] 请求编辑面板
- [ ] 响应展示面板
- [ ] 文件树组件
- [ ] 基础按键绑定
- [ ] 快速请求功能（Ctrl+N）

### Phase 2: 增强功能（2 周）

**目标**：完善用户体验

- [ ] 自适应布局（窄屏优化）
- [ ] 内置编辑器（Ctrl+E）
- [ ] 文件创建功能（Ctrl+T）
- [ ] 语法高亮
- [ ] 历史记录面板
- [ ] 帮助系统

### Phase 3: 高级特性（2-3 周）

**目标**：提升效率与美观

- [ ] 智能补全（变量、指令）
- [ ] 主题系统
- [ ] 多工作区（Tab 管理）
- [ ] JSON 折叠/展开
- [ ] 搜索功能
- [ ] 配置持久化

### Phase 4: 抛光优化（1-2 周）

**目标**：精益求精

- [ ] 性能优化
- [ ] 动画效果
- [ ] 错误处理完善
- [ ] 单元测试
- [ ] 文档补全

---

## 🧪 测试计划

### 单元测试

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_quick_input_parse_url() {
        let input = QuickRequestInput::new("https://api.example.com/users");
        let request = input.parse().unwrap();
        
        assert_eq!(request.method, Method::GET);
        assert_eq!(request.url, "https://api.example.com/users");
    }
    
    #[test]
    fn test_quick_input_parse_curl() {
        let input = QuickRequestInput::new("curl -X POST https://api.example.com/data");
        let request = input.parse().unwrap();
        
        assert_eq!(request.method, Method::POST);
    }
}
```

### 集成测试

- 测试完整的用户工作流
- 测试布局自适应
- 测试文件操作
- 测试按键绑定

---

## 💡 技术难点与解决方案

### 1. 终端兼容性

**问题**：不同终端模拟器的行为差异

**解决方案**：
- 使用 Crossterm 统一抽象层
- 检测终端特性（颜色支持、Unicode）
- 提供降级方案（ASCII fallback）

### 2. 性能优化

**问题**：大文件、长响应的渲染性能

**解决方案**：
- 虚拟滚动（只渲染可见部分）
- 延迟渲染（debounce）
- JSON 流式解析

### 3. 状态管理

**问题**：复杂的 UI 状态同步

**解决方案**：
- 采用 Elm Architecture 单向数据流
- 使用 State Machine 管理模式切换
- 事件驱动更新

---

## 📦 依赖清单

```toml
[dependencies]
# TUI 核心
ratatui = "0.26"
crossterm = "0.27"

# 异步运行时
tokio = { version = "1", features = ["full"] }

# 解析与序列化
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# 工具库
chrono = "0.4"
anyhow = "1"
thiserror = "1"

# 配置管理
config = "0.14"
```

---

## 🚀 快速开始（开发者）

```bash
# 1. 创建 TUI 模块
mkdir -p src/tui/ui/components src/tui/state src/tui/event

# 2. 添加依赖
cargo add ratatui crossterm

# 3. 运行 TUI 模式
cargo run -- tui

# 4. 运行测试
cargo test --lib tui
```

---

## 📚 参考资源

- [Ratatui 官方文档](https://ratatui.rs/)
- [Crossterm 文档](https://docs.rs/crossterm/)
- [K9s TUI 设计](https://github.com/derailed/k9s)
- [Helix Editor](https://github.com/helix-editor/helix)
- [Lazygit](https://github.com/jesseduffield/lazygit)

---

## ✅ 验收标准

TUI 功能完成的验收条件：

1. ✅ 用户可以在 TUI 中完成完整的 HTTP 请求测试流程
2. ✅ 支持直接输入 URL 快速发送请求
3. ✅ 支持创建和编辑 .http 文件
4. ✅ 响应展示美观、易读（语法高亮、格式化）
5. ✅ 在不同终端尺寸下都有良好体验
6. ✅ 所有核心操作都可通过键盘完成
7. ✅ 首次使用无需查阅文档即可上手
8. ✅ 性能流畅（即使处理大响应）

---

**下一步**：等待确认后开始 Phase 1 MVP 开发 🚀
