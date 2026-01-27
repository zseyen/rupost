# RuPost 项目计划

## 1. 需求说明书 (Requirements)

基于 `doc/todo.md` 及项目定位，本项目旨在开发一个基于文档的 API 测试工具（CLI），兼具 `curl`/`httpie` 的即时性与 Postman 的文档管理能力。

### 1.1 核心功能
1.  **命令行请求执行**
    *   支持 `curl` 风格参数解析。
    *   支持 `httpie` 风格参数解析。
    *   支持常见的 HTTP 方法 (GET, POST, etc.)。
    *   支持 Headers, Body, Query Params 设置。
2.  **响应输出与格式化**
    *   打印 Response Headers 和 Body。
    *   支持 JSON 格式化输出 (Pretty Print)。
    *   支持彩色输出 (Syntax Highlighting)。
3.  **文档驱动测试 (核心差异化功能)**
    *   **文件测试**: 支持从文件读取请求定义并执行（类似 `.http` 文件或 `.hurl`）。
    *   **批量执行**: 支持运行一个文件中的多个请求。
    *   **断言/测试**: 支持对响应状态码、Body 内容进行简单断言。
4.  **开发体验 (DX) 增强**
    *   **历史记录**: 保存执行过的请求历史，支持查询和回放。
    *   **环境变量**: 支持定义环境（如 dev, prod），在请求中使用变量（如 `{{base_url}}`）。
    *   **配置管理**: 支持配置文件（`rupost.toml` 或类似），设置默认 Header、超时时间等。
    *   **错误处理**: 统一且友好的错误提示。

## 2. 设计文档 (Design)

### 2.1 架构设计
采用分层架构 (Clean Architecture 思想)：

*   **Interface Layer (CLI)**: `src/cli.rs`, `src/main.rs`
    *   负责参数解析 (`clap`)。
    *   负责命令分发 (Run, Test, History)。
    *   负责输出格式化 (Formatter)。
*   **Application Layer (Service)**: `src/service/` (待创建)
    *   `RequestExecutionService`: 协调请求的构建、发送和结果处理。
    *   `FileParsingService`: 解析请求文件。
    *   `HistoryService`: 管理历史记录。
*   **Domain Layer (Core)**: `src/http/`
    *   `Request`, `Response` 实体定义。
    *   `Client` trait (定义 HTTP 客户端行为，解耦具体实现)。
*   **Infrastructure Layer**:
    *   `HttpClient`: 基于 `reqwest` 的实现。
    *   `Storage`: 基于文件 (SQLite 或 JSON) 的持久化实现。

### 2.2 关键模块设计
1.  **请求文件格式 (File Format)**
    *   **IntelliJ HTTP Client (`.http`)**:
        *   **标准**: 业界通用的纯文本格式，被 JetBrains IDEs、VS Code (REST Client) 广泛支持。
        *   **语法**:
            *   使用 `###` 分隔多个请求。
            *   支持 `# @name` 元数据。
            *   支持变量 `{{var}}`。
            *   支持前置/后置脚本 (Javascript handler)，我们初期可以仅支持简单的断言语法。
    *   **Markdown (`.md`)**:
        *   **支持**: 解析 Markdown 文件中的代码块。
        *   **语法**: 提取标记为 `http` 或 `rest` 的代码块内容进行执行。这对于编写包含文档和可执行示例的 **Literate Programming** 非常有用。
    *   **示例 (.http)**:
        ```http
        ### Login
        POST {{base_url}}/login
        Content-Type: application/json

        { "user": "test" }
        ```
    *   **示例 (.md)**:
        ```markdown
        # API Docs
        Here is how to login:
        ```http
        POST {{base_url}}/login
        ```
        ```
2.  **变量替换**
    *   使用 `{{variable}}` 语法。
    *   在 Request Builder 阶段进行字符串替换。
3.  **历史记录存储**
    *   使用 `dirs` crate 获取系统数据目录。
    *   使用 JSON append-only log 存储历史，简单且易于手动查看/编辑。

## 3. 任务划分 (Task Breakdown)

### Phase 1: 基础建设与 MVP (Basic CLI)
此阶段目标是让 CLI 能跑通，并能正确打印结果。
*   [ ] **Fix**: 修复 `cli.rs` 中同步调用异步 `Client::execute` 的问题，引入 `tokio` runtime。
*   [ ] **Feat**: 实现 Response 的打印输出 (Body, Headers, Status)。
*   [ ] **Feat**: 实现彩色输出和 JSON 格式化 (`utils/formatter.rs`)。
*   [ ] **Refactor**: 统一错误处理 (`anyhow` + `thiserror`)。

### Phase 2: 文档驱动测试 (File-based Testing)
此阶段实现 "Based on documents" 的核心承诺。
*   [ ] **Design**: 确定请求文件解析器 (Parser) 的语法规范 (采用 `.http` 格式)。
*   [ ] **Feat**: 实现文件解析器 (`src/parser/`)，将文本转换为 `Request` 对象。
*   [ ] **Feat**: 实现 `rupost test <file>` 命令，批量执行文件中的请求。
*   [ ] **Feat**: 添加基础断言功能 (例如: assert status == 200)。

### Phase 3: 开发体验增强 (DX Improvements)
*   [ ] **Feat**: 实现历史记录功能 (存储 + `rupost history` 命令)。
*   [ ] **Feat**: 实现环境变量支持 (`env.json` + `{{var}}` 替换)。
*   [ ] **Feat**: 实现配置文件加载 (`config.toml`)。

### Phase 4: 进阶功能 (Advanced)
*   [ ] **Feat**: 数据导入 (如从 Postman collection 导入)。
*   [ ] **Feat**: CI/CD 集成支持 (JUnit report output 等)。
