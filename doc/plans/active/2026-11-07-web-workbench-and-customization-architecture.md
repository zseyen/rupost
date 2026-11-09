# Rupost 活文档工作台 (Web Phase 1) 与全方位定制化架构综合方案

> **文档标识**：2026-11-07-web-workbench-and-customization-architecture  
> **状态**：Active / Ready for Implementation  
> **所属版本**：rupost v0.2.0 (role-alignment-scenario)  
> **指导原则**：Clean Architecture / First Principles / Single-Binary Self-Contained / Zero-Overhead Customization  

---

## 一、 方案全景与核心定位

`rupost web` 的本质是**本地优先（Local-first）的高性能活文档编写助手与原位联调工作台**，而非臃肿的云端 API 测试平台。它在坚守“单一二进制交付、毫秒级响应、零外部网络依赖”的前提下，向三类目标用户提供统一的能力支撑：

1. **轻量本地开发者**：直接运行 `rupost web`，零配置享受现代化 HSL 暗黑活文档浏览、多格式向导装入器（支持 cURL / HTTPie / HAR / OpenAPI / Postman 智能转换）、保存前语法守卫与原位单块调试发包（含时延瀑布图）。
2. **定制前端团队**：通过 `--static-dir ./dist` 选项直接挂载企业自建的前端 SPA（React/Vue/Tailwind 等），利用标准的 Headless OpenAPI 契约无缝驱动核心引擎。
3. **平台架构师 / 微服务系统**：将 Rupost 作为 Rust Crate 依赖引入，通过 `create_web_router` 纯函数生成 Axum 子路由，并注入自定义的 `DocumentStore`（如 Git / 数据库存储）与统一企业 SSO 鉴权中间件。

---

## 二、 四层定制化架构解耦设计 (The 4-Layer Pluggable Architecture)

依据 Clean Architecture，系统由内向外彻底解耦，核心引擎（Runner / Parser / Mock）对 Web 展现层完全无感知：

```
 ┌────────────────────────────────────────────────────────┐
 │                      自定义前端                         │
 │   (官方轻量 UI  OR  用户自研 React/Vue  OR  VSCode 插件) │
 └──────────────────────────┬─────────────────────────────┘
                            │ 标准 REST + SSE 契约 (/api/v1/*)
 ┌──────────────────────────▼─────────────────────────────┐
 │            rupost::web::create_web_router              │
 │  ┌───────────────────────┐  ┌───────────────────────┐  │
 │  │ dyn AssetProvider     │  │ dyn DocumentStore     │  │
 │  │ (静态资产解耦 Trait)   │  │ (存储持久化解耦 Trait)  │  │
 │  └───────────────────────┘  └───────────────────────┘  │
 └──────────────────────────┬─────────────────────────────┘
                            │ 纯领域调度 (Clean Architecture)
 ┌──────────────────────────▼─────────────────────────────┐
 │        Rupost Core Engine (Runner / Mock / Parser)     │
 └────────────────────────────────────────────────────────┘
```

### 1. 静态资产解耦：`AssetProvider` Trait
允许替换官方默认的前端 HTML/JS/CSS 产物：
```rust
pub trait AssetProvider: Send + Sync + 'static {
    /// 根据相对路径获取资产字节流与 Content-Type
    fn get_asset(&self, path: &str) -> Option<(std::borrow::Cow<'static, [u8]>, &'static str)>;
}

// 核心实现：
// - EmbeddedAssetProvider: 编译期内嵌官方轻量 UI (零 node 依赖)
// - LocalDirAssetProvider: 外部物理目录托管 (--static-dir 激活)
```

### 2. 存储与数据源解耦：`DocumentStore` Trait
解耦本地磁盘文件系统，支持对接远程 Git、知识库或云数据库：
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub modified_at: Option<String>,
}

#[async_trait::async_trait]
pub trait DocumentStore: Send + Sync + 'static {
    async fn list_documents(&self, subpath: Option<&str>) -> crate::Result<Vec<FileEntry>>;
    async fn read_document(&self, path: &str) -> crate::Result<String>;
    async fn save_document(&self, path: &str, content: &str) -> crate::Result<()>;
}

// 核心实现：
// - LocalFsDocumentStore: 本地文件读写 (严格内置沙箱与防目录穿越校验)
```

### 3. 服务配置与路由生成器：`create_web_router`
供外部宿主 Rust 项目（如企业内部门户网关）以子路由无缝嵌入：
```rust
#[derive(Clone)]
pub struct WebServerConfig {
    pub title: String,
    pub readonly_mode: bool,                     // 只读模式 (阻断 save)
    pub allow_external_hosts: bool,              // 是否允许发包至外网
    pub document_store: Arc<dyn DocumentStore>,  // 可插拔存储
    pub asset_provider: Arc<dyn AssetProvider>,  // 可插拔前端资产
    pub middlewares: Vec<ExecutorMiddleware>,    // 执行流中间件
}

/// 导出纯净的 Axum 路由 (调用者自主绑定端口或嵌套到现有 App)
pub fn create_web_router(config: WebServerConfig) -> axum::Router;
```

---

## 三、 Headless OpenAPI / REST & SSE 标准交互契约

所有前端（官方 UI 或用户自定义前端）均通过统一的版本化接口驱动：

| API 端点 | Method | 功能描述 | 核心输入 / 输出规范 |
| :--- | :---: | :--- | :--- |
| `/api/v1/files` | `GET` | 遍历文档目录树 | `output: { files: Vec<FileEntry> }` |
| `/api/v1/file` | `GET` | 读取文档原文 | `query: ?path=...` -> `output: { content: string, line_count: usize }` |
| `/api/v1/save` | `POST` | 保存文档（前置语法守卫） | `input: { path: string, content: string }`<br>守卫失败返回 `422` 与行号 |
| `/api/v1/run/block` | `POST` | **原位单块调试发包** | `input: RunBlockRequest` -> `output: RunBlockResponse` |
| `/api/v1/run/suite` | `POST` | 整文件套件批量执行 | `input: { path: string, env: Option<string> }` |
| `/api/v1/mock/simulate` | `POST` | **Mock 规则命中仿真器** | `input: { probe: MockProbe, variants: [...] }` -> `output: MatchSimulation` |
| `/api/v1/import` | `POST` | **多格式向导装入器 (统一开闭入口)** | `input: ImportRequest` -> `output: ImportResponse`<br>支持 `cURL`, `HTTPie`, `HAR`, `OpenAPI`, `Postman`, `Raw HTTP` 智能嗅探与转换 |
| `/api/v1/import/curl` | `POST` | cURL 装入 (兼容别名) | 内部透传至 `/api/v1/import` (format: curl) |
| `/api/v1/events/stream` | `GET` | **SSE 实时事件流** | 单向推流 `RunEvent`（Token 增量、逐帧断言、时延指标） |

### 核心交互模型与多格式导入契约：

```rust
/// 支持装入的外部源格式枚举（开闭原则）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportFormat {
    Auto,      // 智能嗅探（根据前缀和特征自动识别）
    Curl,      // cURL 命令行
    Httpie,    // HTTPie 命令行
    Har,       // 浏览器 Network 导出的 HAR 归档文件
    OpenApi,   // OpenAPI 3.0/3.1 或 Swagger 2.0 (JSON/YAML)
    Postman,   // Postman Collection v2.1
    RawHttp,   // RFC 7230 原始 HTTP 报文
}

/// 统一装入请求契约
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportRequest {
    #[serde(default = "default_import_format")]
    pub format: ImportFormat,         // 导入格式 (默认 auto 嗅探)
    pub content: String,               // 待导入的原始指令或文本
    pub templatize_host: bool,         // 是否自动提取域名并转写为 {{base_url}}
    pub generate_mock_placeholder: bool,// 是否自动补齐标准 @mock-default 占位
}

fn default_import_format() -> ImportFormat {
    ImportFormat::Auto
}

/// 统一装入响应契约
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResponse {
    pub detected_format: ImportFormat,
    pub markdown_blocks: Vec<String>,  // 转换出的 Markdown 代码块列表
    pub total_imported: usize,
    pub warnings: Vec<String>,
}

/// 格式导入适配器 Trait（多格式扩展点）
pub trait RequestImporter: Send + Sync {
    fn format(&self) -> ImportFormat;
    fn can_handle(&self, content: &str) -> bool;
    fn import(&self, content: &str, templatize_host: bool) -> crate::Result<Vec<crate::parser::ParsedRequest>>;
}
```
```rust
/// 原位单块发包入参
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunBlockRequest {
    pub file_path: String,
    pub raw_block: String,            // 当前编辑的代码块文本
    pub env: Option<String>,          // 执行环境 (如 staging)
    pub extra_variables: Option<HashMap<String, String>>,
}

/// 原位发包出参 (复用 Stage 2 归一化时延模型)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunBlockResponse {
    pub success: bool,
    pub status: Option<u16>,
    pub duration_ms: u64,
    pub assertions: Vec<ReportAssertionItem>,
    pub timing_waterfall: Option<TimingWaterfall>, // 5 阶段瀑布图 (DNS/TCP/TLS/TTFB/Transfer)
    pub response_headers: HashMap<String, String>,
    pub response_body: Option<String>,
    pub error: Option<String>,
}
```

---

## 四、 四大预设业务场景实战推演

### 场景 1：极简开发者本地即刻使用 (Zero-Config Local Loop)
*   **操作**：`rupost web`
*   **行为**：自动启动 `127.0.0.1:19190`，加载当前目录 `.md`/`.http`，浏览器展现内置高质感暗黑工作台。开发者修改断言，触发语法守卫通过后落盘；点按“单块调试”，右侧实时渲染响应体与细粒度时延瀑布图。

### 场景 2：企业前端团队定制专用 UI (Custom SPA via `--static-dir`)
*   **操作**：`rupost web --static-dir ./custom-ui/dist --port 3000`
*   **行为**：Rupost 激活 `LocalDirAssetProvider`，托管企业统一设计风格（如 Ant Design / Tailwind）的 React/Vue 前端。自研前端通过 `fetch('/api/v1/run/block')` 调度底层引擎，实现业务深定制。

### 场景 3：云原生平台嵌入 (Headless Rust Crate Embedding)
*   **操作**：宿主 Rust 服务引入 `rupost = { path = "..." }`，实现 `CompanyGitDocumentStore`；
*   **行为**：调用 `rupost::web::create_web_router(config)` 生成 Axum 子路由，挂载到 `app.nest("/api/rupost", router).layer(CompanySsoLayer::new())`，文档自动拉取自 GitLab，测试结果回传内部效能看板。

### 场景 4：CI/CD 自动化预览与演示保护 (Read-Only Showcase)
*   **操作**：`rupost web --readonly --env staging --host 0.0.0.0 --port 8080`
*   **行为**：供团队在线审阅 PR 文档与发起原位发包测试；若尝试调用 `POST /api/v1/save`，后端强制拦截并返回 `403 Forbidden: Server is running in read-only mode`，彻底保护生产代码库。

---

## 五、 安全与健壮性防御体系

1. **防路径穿越 (Path Traversal Guard)**：`LocalFsDocumentStore` 对所有请求路径执行 `canonicalize()`，强制校验必须属于当前工作目录子树，越界即阻断。
2. **保存前语法守卫 (Syntax Guard & AST Linting)**：在写盘前调用 `MarkdownFileParser::parse_string` 进行 AST 解析校验，非标准指令或语法崩溃阻断写入并返回 `422` 错误行号。
3. **防 XSS 注入 (HTML Entity Escape & Zero InnerHTML)**：所有响应字段经过 `escape_html` 过滤，内置前端纯基于 DOM 属性操作。
4. **绑定安全与开发跨域 (Binding & CORS Policy)**：默认仅监听 `127.0.0.1`；提供 `--host` 显式对外开放，`--dev` 模式下智能放开前端常用调试端口的跨域访问。

---

## 六、 分阶段 TDD 实施路线与交付里程碑

```
┌────────────────────────────────────────────────────────────────────────┐
│ Stage 3: Web Phase 1 活文档工作台                                       │
│                                                                        │
│ Step 1 (领域端点与接口抽象):                                             │
│   - 实现 AssetProvider、DocumentStore、WebServerConfig 接口             │
│   - 实现 LocalFsDocumentStore (内置 Path Traversal 校验)               │
│   - 实现 axum 0.8 路由分发器 create_web_router                         │
│   - 实现 6 个核心 REST 端点 (files, file, save 语法守卫, run/block,    │
│     mock/simulate, import 多格式装入器)                                │
│   - 编写 tests/web_api_integration_test.rs 保证 100% PASS              │
│                                                                        │
│ Step 2 (嵌入式自包含前端工作台):                                        │
│   - 编写自包含 index.html / app.js / style.css (Vanilla ESM)           │
│   - 集成文件导航树、多格式向导装入器 (cURL/HTTPie/HAR/OpenAPI) 与单块原位发包调试面板 │
│   - 复用 Stage 2 归一化时延瀑布图 (TimingWaterfall)                    │
│   - 接入 --static-dir 与 --readonly 选项                               │
└────────────────────────────────────┬───────────────────────────────────┘
                                     │
                                     ▼
┌────────────────────────────────────────────────────────────────────────┐
│ Stage 4: 脚手架补齐与全量三层质量验收                                   │
│                                                                        │
│ Step 3 (脚手架模板补齐):                                               │
│   - 在 src/template/strategy.rs 中实现 SpecTemplateStrategy            │
│   - 提供 rupost init spec 命令，生成标准电商与 IAM 契约文档             │
│   - 编写 template_test 验证文件生成合规性                              │
│                                                                        │
│ Step 4 (全量质量回归与原子化归档):                                     │
│   - 统一收拢 examples/ 契约用例                                        │
│   - 执行 tests/verify_features.sh (冒烟测试)                           │
│   - 执行 examples/run_all.sh (全特性长跑)                              │
│   - cargo clippy 零告警，cargo fmt 合规，jj 原子化提交归档              │
└────────────────────────────────────────────────────────────────────────┘
```

---

## 七、 验收标准 (Definition of Done)

- [ ] **接口抽象**：`AssetProvider` 与 `DocumentStore` 抽象清晰，支持第三方无缝替换。
- [ ] **安全保障**：路径穿越、只读拦截、语法错误阻断在集成测试中有明确断言保护。
- [ ] **原位调试**：`POST /api/v1/run/block` 能够精准返回状态码、断言结果及 5 阶段时延瀑布条。
- [ ] **单二进制自包含**：编译发布无任何外部静态文件或 node 依赖，体积增量 < 200KB。
- [ ] **质量回归**：全量单元测试与集成测试全部 PASS，覆盖率不降低。
