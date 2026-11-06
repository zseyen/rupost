# TUI 插件接口设计

> **核心问题**：在 TUI 架构中需要预留哪些插件接口？为什么需要预留？

---

## 🎯 要不要预留插件接口？

**答案：必须预留！**

### 原因分析

1. **项目规划明确**：插件系统是 RuPost 的第一优先级功能
2. **架构成本**：后期重构 TUI 架构的成本远高于前期设计
3. **扩展性需求**：未来功能如 WebSocket、GraphQL、AI 集成都需要插件化
4. **避免耦合**：良好的插件架构能保持核心代码简洁

### 不预留的后果

❌ 核心代码与功能代码高度耦合  
❌ 每次新增功能都需要修改核心 TUI 代码  
❌ 难以维护第三方扩展  
❌ 重构成本呈指数级增长  

---

## 🏗️ 需要预留的核心接口

基于 **开闭原则**（对扩展开放，对修改封闭），我们需要在以下 7 个关键点预留插件接口：

---

## 1️⃣ UI 组件扩展接口（Widget Plugin）

### 为什么需要？

插件可能需要：
- 添加自定义面板（如 WebSocket 监控面板、GraphQL Schema 浏览器）
- 在现有面板中插入自定义组件（如响应解密器、数据可视化图表）
- 创建浮动窗口或通知组件

### 接口设计

```rust
// src/tui/plugin/widget.rs

use ratatui::{Frame, layout::Rect};

/// UI 组件插件 Trait
pub trait WidgetPlugin: Send + Sync {
    /// 插件名称
    fn name(&self) -> &str;
    
    /// 组件标识（用于布局管理）
    fn id(&self) -> &str;
    
    /// 渲染组件
    fn render(&mut self, frame: &mut Frame, area: Rect);
    
    /// 处理输入事件
    fn handle_event(&mut self, event: &Event) -> PluginResult<EventResponse>;
    
    /// 组件优先级（决定渲染顺序）
    fn priority(&self) -> i32 { 0 }
    
    /// 是否可见
    fn is_visible(&self) -> bool { true }
}

/// 插件注册器
pub struct WidgetRegistry {
    plugins: HashMap<String, Box<dyn WidgetPlugin>>,
    layout_config: LayoutConfig,
}

impl WidgetRegistry {
    /// 注册插件
    pub fn register(&mut self, plugin: Box<dyn WidgetPlugin>) {
        self.plugins.insert(plugin.id().to_string(), plugin);
    }
    
    /// 获取所有可见插件
    pub fn visible_plugins(&self) -> Vec<&dyn WidgetPlugin> {
        self.plugins.values()
            .filter(|p| p.is_visible())
            .map(|p| p.as_ref())
            .collect()
    }
}
```

### 使用示例

```rust
// 插件示例：WebSocket 监控面板
pub struct WebSocketMonitor {
    id: String,
    connections: Vec<WsConnection>,
}

impl WidgetPlugin for WebSocketMonitor {
    fn name(&self) -> &str { "WebSocket Monitor" }
    fn id(&self) -> &str { &self.id }
    
    fn render(&mut self, frame: &mut Frame, area: Rect) {
        // 渲染 WebSocket 连接列表
        let list = List::new(self.connections.iter().map(|c| {
            Line::from(format!("{} - {}", c.url, c.status))
        }));
        frame.render_widget(list, area);
    }
    
    fn handle_event(&mut self, event: &Event) -> PluginResult<EventResponse> {
        // 处理 WebSocket 相关事件
        Ok(EventResponse::Handled)
    }
}
```

---

## 2️⃣ 请求/响应处理管道（Middleware Plugin）

### 为什么需要？

插件需要能够：
- 在请求发送前修改（如加密、签名、添加 trace ID）
- 在响应接收后处理（如解密、数据脱敏、性能分析）
- 拦截特定类型的请求（如 GraphQL、gRPC）

### 接口设计

```rust
// src/tui/plugin/middleware.rs

/// 请求中间件
#[async_trait]
pub trait RequestMiddleware: Send + Sync {
    /// 插件名称
    fn name(&self) -> &str;
    
    /// 优先级（数字越小越先执行）
    fn priority(&self) -> i32 { 100 }
    
    /// 处理请求（可修改请求）
    async fn process_request(
        &self,
        request: &mut Request,
        context: &PluginContext,
    ) -> PluginResult<()>;
}

/// 响应中间件
#[async_trait]
pub trait ResponseMiddleware: Send + Sync {
    fn name(&self) -> &str;
    fn priority(&self) -> i32 { 100 }
    
    /// 处理响应（可修改响应）
    async fn process_response(
        &self,
        response: &mut Response,
        context: &PluginContext,
    ) -> PluginResult<()>;
}

/// 中间件管道
pub struct MiddlewarePipeline {
    request_middlewares: Vec<Box<dyn RequestMiddleware>>,
    response_middlewares: Vec<Box<dyn ResponseMiddleware>>,
}

impl MiddlewarePipeline {
    pub fn add_request_middleware(&mut self, mw: Box<dyn RequestMiddleware>) {
        self.request_middlewares.push(mw);
        self.request_middlewares.sort_by_key(|m| m.priority());
    }
    
    pub async fn execute_request(
        &self,
        mut request: Request,
        context: &PluginContext,
    ) -> PluginResult<Request> {
        for middleware in &self.request_middlewares {
            middleware.process_request(&mut request, context).await?;
        }
        Ok(request)
    }
}
```

### 使用示例

```rust
// 插件示例：自动添加 Trace ID
pub struct TraceIdMiddleware;

#[async_trait]
impl RequestMiddleware for TraceIdMiddleware {
    fn name(&self) -> &str { "Trace ID Injector" }
    fn priority(&self) -> i32 { 10 } // 高优先级
    
    async fn process_request(
        &self,
        request: &mut Request,
        _context: &PluginContext,
    ) -> PluginResult<()> {
        let trace_id = uuid::Uuid::new_v4().to_string();
        request.headers.insert(
            "X-Trace-ID".to_string(),
            trace_id,
        );
        Ok(())
    }
}
```

---

## 3️⃣ 命令与快捷键扩展（Command Plugin）

### 为什么需要？

插件需要：
- 注册自定义命令（如 `:graphql-schema`、`:ws-connect`）
- 绑定快捷键（如 `Ctrl+G` 打开 GraphQL 面板）
- 扩展命令面板功能

### 接口设计

```rust
// src/tui/plugin/command.rs

/// 命令插件
pub trait CommandPlugin: Send + Sync {
    /// 命令名称（如 "websocket:connect"）
    fn command_name(&self) -> &str;
    
    /// 命令别名
    fn aliases(&self) -> Vec<&str> { vec![] }
    
    /// 命令描述（用于帮助）
    fn description(&self) -> &str;
    
    /// 执行命令
    fn execute(
        &self,
        args: &[String],
        context: &mut AppState,
    ) -> PluginResult<CommandResult>;
    
    /// 自动补全
    fn complete(&self, partial: &str) -> Vec<String> {
        vec![]
    }
}

/// 快捷键绑定
pub struct KeyBinding {
    pub key: KeyCode,
    pub modifiers: KeyModifiers,
    pub command: String,
}

pub trait KeyBindingPlugin: Send + Sync {
    /// 注册的快捷键
    fn bindings(&self) -> Vec<KeyBinding>;
}

/// 命令注册器
pub struct CommandRegistry {
    commands: HashMap<String, Box<dyn CommandPlugin>>,
    key_bindings: Vec<(KeyBinding, Box<dyn CommandPlugin>)>,
}
```

### 使用示例

```rust
// 插件示例：GraphQL Schema 查看器
pub struct GraphQLSchemaCommand;

impl CommandPlugin for GraphQLSchemaCommand {
    fn command_name(&self) -> &str { "graphql:schema" }
    fn aliases(&self) -> Vec<&str> { vec!["gql", "schema"] }
    fn description(&self) -> &str { "查看 GraphQL Schema" }
    
    fn execute(
        &self,
        args: &[String],
        context: &mut AppState,
    ) -> PluginResult<CommandResult> {
        // 获取并显示 Schema
        let url = args.get(0).ok_or("需要提供 GraphQL 端点 URL")?;
        let schema = fetch_schema(url)?;
        context.ui.show_custom_panel(Box::new(SchemaViewer::new(schema)));
        Ok(CommandResult::Success)
    }
}
```

---

## 4️⃣ 数据解析与格式化（Parser Plugin）

### 为什么需要？

需要支持：
- 自定义数据格式（Protobuf、MessagePack、YAML）
- 语法高亮扩展
- 自定义断言语法

### 接口设计

```rust
// src/tui/plugin/parser.rs

/// 内容解析器插件
pub trait ParserPlugin: Send + Sync {
    /// 支持的 Content-Type
    fn supported_content_types(&self) -> Vec<&str>;
    
    /// 解析内容
    fn parse(&self, content: &[u8]) -> PluginResult<ParsedContent>;
    
    /// 格式化输出
    fn format(&self, content: &ParsedContent) -> String;
    
    /// 语法高亮
    fn highlight(&self, content: &str) -> Vec<Span>;
}

/// 解析后的内容
pub enum ParsedContent {
    Json(serde_json::Value),
    Xml(xmltree::Element),
    Custom(Box<dyn Any + Send + Sync>),
}

/// 解析器注册
pub struct ParserRegistry {
    parsers: HashMap<String, Box<dyn ParserPlugin>>,
}

impl ParserRegistry {
    pub fn parse_response(&self, response: &Response) -> PluginResult<ParsedContent> {
        let content_type = response.content_type();
        if let Some(parser) = self.parsers.get(content_type) {
            parser.parse(&response.body)
        } else {
            Err(PluginError::UnsupportedContentType)
        }
    }
}
```

---

## 5️⃣ 状态与数据存储（State Plugin）

### 为什么需要？

插件需要：
- 持久化自己的配置和数据
- 访问共享状态（如当前环境、变量）
- 订阅状态变更事件

### 接口设计

```rust
// src/tui/plugin/state.rs

/// 插件上下文（提供访问核心功能）
pub struct PluginContext {
    /// 变量访问
    pub variables: Arc<RwLock<VariableContext>>,
    
    /// 历史记录
    pub history: Arc<RwLock<History>>,
    
    /// 配置
    pub config: Arc<RwLock<Config>>,
    
    /// 插件存储路径
    plugin_data_dir: PathBuf,
}

impl PluginContext {
    /// 获取插件专属存储路径
    pub fn plugin_storage(&self, plugin_id: &str) -> PathBuf {
        self.plugin_data_dir.join(plugin_id)
    }
    
    /// 保存插件数据
    pub fn save_data<T: Serialize>(
        &self,
        plugin_id: &str,
        key: &str,
        data: &T,
    ) -> PluginResult<()> {
        let path = self.plugin_storage(plugin_id).join(format!("{}.json", key));
        let content = serde_json::to_string_pretty(data)?;
        fs::write(path, content)?;
        Ok(())
    }
    
    /// 读取插件数据
    pub fn load_data<T: DeserializeOwned>(
        &self,
        plugin_id: &str,
        key: &str,
    ) -> PluginResult<T> {
        let path = self.plugin_storage(plugin_id).join(format!("{}.json", key));
        let content = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&content)?)
    }
}

/// 状态订阅
pub trait StateObserver: Send + Sync {
    fn on_variable_changed(&self, name: &str, value: &str);
    fn on_environment_changed(&self, env: &str);
    fn on_request_completed(&self, response: &Response);
}
```

---

## 6️⃣ 生命周期钩子（Lifecycle Plugin）

### 为什么需要？

插件需要：
- 应用启动时初始化
- 请求前后执行操作
- 应用关闭时清理资源

### 接口设计

```rust
// src/tui/plugin/lifecycle.rs

#[async_trait]
pub trait LifecyclePlugin: Send + Sync {
    /// 插件 ID（唯一标识）
    fn id(&self) -> &str;
    
    /// 插件元数据
    fn metadata(&self) -> PluginMetadata;
    
    /// 应用启动时调用
    async fn on_startup(&mut self, context: &PluginContext) -> PluginResult<()> {
        Ok(())
    }
    
    /// 应用关闭时调用
    async fn on_shutdown(&mut self, context: &PluginContext) -> PluginResult<()> {
        Ok(())
    }
    
    /// 请求发送前
    async fn before_request(
        &self,
        request: &Request,
        context: &PluginContext,
    ) -> PluginResult<()> {
        Ok(())
    }
    
    /// 请求完成后
    async fn after_request(
        &self,
        request: &Request,
        response: &Response,
        context: &PluginContext,
    ) -> PluginResult<()> {
        Ok(())
    }
}

/// 插件元数据
pub struct PluginMetadata {
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub dependencies: Vec<String>,
}
```

---

## 7️⃣ 配置与设置扩展（Settings Plugin）

### 为什么需要？

插件需要：
- 在设置面板中添加自己的配置项
- 读取用户配置
- 验证配置有效性

### 接口设计

```rust
// src/tui/plugin/settings.rs

/// 配置插件
pub trait SettingsPlugin: Send + Sync {
    /// 配置项定义
    fn settings_schema(&self) -> Vec<SettingDefinition>;
    
    /// 渲染配置 UI
    fn render_settings(&self, frame: &mut Frame, area: Rect);
    
    /// 加载配置
    fn load_config(&mut self, config: &serde_json::Value) -> PluginResult<()>;
    
    /// 保存配置
    fn save_config(&self) -> serde_json::Value;
    
    /// 验证配置
    fn validate(&self, config: &serde_json::Value) -> PluginResult<()>;
}

/// 配置项定义
pub struct SettingDefinition {
    pub key: String,
    pub label: String,
    pub description: String,
    pub setting_type: SettingType,
    pub default_value: serde_json::Value,
}

pub enum SettingType {
    String,
    Number,
    Boolean,
    Select { options: Vec<String> },
    File,
    Color,
}
```

---

## 🔌 插件加载机制

### 插件管理器

```rust
// src/tui/plugin/manager.rs

pub struct PluginManager {
    /// 已加载的插件
    plugins: HashMap<String, Box<dyn LifecyclePlugin>>,
    
    /// UI 组件注册器
    widget_registry: WidgetRegistry,
    
    /// 中间件管道
    middleware_pipeline: MiddlewarePipeline,
    
    /// 命令注册器
    command_registry: CommandRegistry,
    
    /// 解析器注册器
    parser_registry: ParserRegistry,
    
    /// 插件上下文
    context: Arc<PluginContext>,
}

impl PluginManager {
    /// 加载插件
    pub async fn load_plugin(&mut self, plugin: Box<dyn LifecyclePlugin>) -> PluginResult<()> {
        let plugin_id = plugin.id().to_string();
        
        // 调用启动钩子
        plugin.on_startup(&self.context).await?;
        
        // 注册各类扩展
        self.register_plugin_extensions(&plugin)?;
        
        // 保存插件引用
        self.plugins.insert(plugin_id, plugin);
        
        Ok(())
    }
    
    /// 卸载插件
    pub async fn unload_plugin(&mut self, plugin_id: &str) -> PluginResult<()> {
        if let Some(mut plugin) = self.plugins.remove(plugin_id) {
            plugin.on_shutdown(&self.context).await?;
        }
        Ok(())
    }
    
    /// 发现并加载所有插件
    pub async fn discover_plugins(&mut self) -> PluginResult<()> {
        let plugin_dir = self.context.plugin_data_dir.join("installed");
        
        for entry in fs::read_dir(plugin_dir)? {
            let path = entry?.path();
            if path.extension() == Some("wasm".as_ref()) {
                // 加载 WASM 插件
                self.load_wasm_plugin(&path).await?;
            }
        }
        
        Ok(())
    }
}
```

---

## 📦 插件分发格式

### 建议使用 WebAssembly（WASM）

**原因**：
- ✅ 跨平台（无需为不同操作系统编译）
- ✅ 安全沙箱（限制插件权限）
- ✅ 性能接近原生
- ✅ 生态成熟（wasmtime、wasmer）

### 插件目录结构

```
~/.rupost/plugins/
├── installed/
│   ├── websocket-monitor/
│   │   ├── plugin.wasm
│   │   ├── manifest.toml
│   │   └── assets/
│   └── graphql-explorer/
│       ├── plugin.wasm
│       └── manifest.toml
└── config/
    ├── websocket-monitor.toml
    └── graphql-explorer.toml
```

### Manifest 格式

```toml
# manifest.toml
[plugin]
id = "websocket-monitor"
name = "WebSocket Monitor"
version = "1.0.0"
author = "Your Name"
description = "实时监控 WebSocket 连接"

[dependencies]
rupost = ">=1.0.0"

[permissions]
network = true  # 是否允许网络访问
filesystem = ["~/.rupost/plugins/websocket-monitor"]  # 文件访问权限
```

---

## 🛡️ 安全考虑

### 权限系统

```rust
// src/tui/plugin/permissions.rs

pub struct PluginPermissions {
    /// 允许网络访问
    pub network: bool,
    
    /// 允许的文件系统路径
    pub filesystem: Vec<PathBuf>,
    
    /// 允许执行外部命令
    pub execute_commands: bool,
    
    /// 允许访问的环境变量
    pub env_vars: Vec<String>,
}

impl PluginManager {
    fn check_permission(
        &self,
        plugin_id: &str,
        permission: Permission,
    ) -> PluginResult<()> {
        let manifest = self.load_manifest(plugin_id)?;
        
        if !manifest.permissions.allows(&permission) {
            return Err(PluginError::PermissionDenied {
                plugin: plugin_id.to_string(),
                permission,
            });
        }
        
        Ok(())
    }
}
```

---

## 🚀 实施建议

### Phase 1: 核心接口（MVP 阶段同步）

- [ ] 定义基础 Trait（WidgetPlugin、LifecyclePlugin）
- [ ] 实现 PluginManager 框架
- [ ] 实现 PluginContext
- [ ] 添加插件加载/卸载机制

### Phase 2: 扩展能力（v0.2）

- [ ] 中间件管道
- [ ] 命令注册
- [ ] 快捷键绑定
- [ ] 状态订阅

### Phase 3: WASM 支持（v0.3）

- [ ] 集成 wasmtime
- [ ] WASM 插件加载器
- [ ] 权限沙箱
- [ ] 插件市场准备

---

## 💡 最佳实践建议

### 1. 从第一天就考虑插件
即使暂时不实现，也要保持架构的可扩展性：

```rust
// ❌ 不好的设计
pub struct ResponsePanel {
    content: String,
}

// ✅ 好的设计
pub struct ResponsePanel {
    content: String,
    custom_renderers: Vec<Box<dyn ContentRenderer>>,  // 预留扩展点
}
```

### 2. 使用依赖注入
```rust
pub struct App {
    plugin_manager: Arc<PluginManager>,
}

impl App {
    pub fn new(plugin_manager: Arc<PluginManager>) -> Self {
        Self { plugin_manager }
    }
}
```

### 3. 事件驱动架构
所有关键操作都应该发出事件，让插件可以监听：

```rust
pub enum AppEvent {
    RequestSent(Request),
    ResponseReceived(Response),
    FileOpened(PathBuf),
    // ...
}

impl App {
    fn send_request(&mut self, request: Request) {
        // 发出事件
        self.emit_event(AppEvent::RequestSent(request.clone()));
        
        // 执行请求
        let response = self.http_client.send(request);
        
        // 发出完成事件
        self.emit_event(AppEvent::ResponseReceived(response));
    }
}
```

---

## 📊 总结对比

| 接口类型 | 是否必须 | 优先级 | 复杂度 | 实施阶段 |
|---------|---------|--------|--------|---------|
| UI 组件扩展 | ✅ 必须 | 🔴 高 | ⭐⭐⭐ | Phase 1 |
| 请求/响应管道 | ✅ 必须 | 🔴 高 | ⭐⭐⭐⭐ | Phase 2 |
| 命令扩展 | ✅ 必须 | 🟡 中 | ⭐⭐ | Phase 2 |
| 数据解析器 | 🟢 推荐 | 🟡 中 | ⭐⭐⭐ | Phase 2 |
| 状态存储 | ✅ 必须 | 🔴 高 | ⭐⭐ | Phase 1 |
| 生命周期钩子 | ✅ 必须 | 🔴 高 | ⭐⭐ | Phase 1 |
| 配置扩展 | 🟢 推荐 | 🔵 低 | ⭐⭐ | Phase 3 |

---

## ✅ 核心建议

**现在就应该做的**：
1. ✅ 定义插件 Trait 接口（即使暂不实现）
2. ✅ 使用依赖注入设计 TUI 组件
3. ✅ 在关键位置预留扩展点（如 `Vec<Box<dyn Plugin>>`）
4. ✅ 采用事件驱动架构

**可以延后的**：
- WASM 加载器实现
- 权限沙箱
- 插件市场

**关键原则**：
> **现在设计接口，未来实现功能。接口稳定比实现完整更重要。**

---

下一步我可以：
1. 创建插件接口的代码实现（Rust Trait 定义）
2. 修改 TUI 实施方案，整合插件架构
3. 提供一个完整的插件开发示例

您希望我继续哪个方向？
