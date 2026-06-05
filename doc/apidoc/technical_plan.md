# RuPost 由于本地文档生成 (Local API Doc) 技术方案

## 1. 概述
本方案旨在将 RuPost 打造为“API 文档生成器”。通过解析项目中的 `.http` 文件，提取元数据、请求定义的和 Schema 信息，自动生成标准化的 **OpenAPI 3.1 (Swagger)** 规范文件，并基于此生成美观的 HTML 静态文档网站。

**核心理念**: **Single Source of Truth** (SSOT)。代码（`.http`）即文档，杜绝文档与实现脱节。

## 2. 详细技术方案

### 2.1 总体架构

```mermaid
graph TD
    UserInput[.http Files] -->|Glob Scan| Collector[File Collector]
    
    subgraph "Parser Core"
        Collector -->|Raw String| Lexer[RuPost Lexer]
        Lexer -->|Tokens| Parser[RuPost Parser]
        Parser -->|AST| Analyzer[Static Analyzer]
    end
    
    subgraph "Doc Engine (doc/apidoc)"
        Analyzer -->|Request Structs| OpenApiGenerator[OpenAPI Generator]
        
        OpenApiGenerator -->|Extract| Meta[Annotations (@desc, @tag)]
        OpenApiGenerator -->|Type Inference| BodySchema[Body Schema Builder]
        
        BodySchema -->|Merge| SwaggerObj[OpenAPI 3.1 Object]
    end
    
    SwaggerObj -->|Serialize| JsonSpec[openapi.json]
    
    subgraph "Render Engine"
        JsonSpec -->|Inject| HtmlTemplate[Redoc/SwaggerUI Template]
        HtmlTemplate -->|Serve/Build| StaticSite[Static HTML]
    end
```

### 2.2 模块详细设计

#### A. 静态分析器 (Static Analyzer)
现有的 Parser 主要服务于“执行”，新模块需要服务于“文档”。
需要增强 Parser 对**注释块 (Comment Block)** 的提取能力。

*   **输入**: `.http` 文件内容。
*   **处理**: 识别 `###` 分隔符前的注释块。
*   **支持指令**:
    *   `@name`: API 唯一标识 ID。
    *   `@desc`: Markdown 格式的详细描述。
    *   `@tag`: 分组标签 (e.g., `User`, `Admin`).
    *   `@ref`: 引用外部文件 (e.g., `Body < ./user_schema.json`).

#### B. 类型推导引擎 (Schema Builder)
这是最难也是最有价值的部分。
需要一个 `JsonToSchema` 转换器。

*   **输入**: JSON Body (e.g., `{"name": "Alice", "age": 18}`)
*   **输出**: JSON Schema (Draft 2020-12)
    ```json
    {
      "type": "object",
      "properties": {
        "name": { "type": "string" },
        "age": { "type": "integer" }
      }
    }
    ```
*   **递归处理**: 支持嵌套对象和数组的推导。

#### C. OpenAPI 生成器 (Generator)
利用 Rust 的 `utoipa` 或 `openapiv3` crate 构建 OpenAPI 结构体。
将分析出的 Request/Response 映射到 `PathItem` 和 `Operation` 对象。

### 2.3 生成目标
1.  **JSON**: `rupost doc export --format openapi` -> 输出 `openapi.json`。
2.  **HTML**: `rupost doc build` -> 输出包含 `openapi.json` 数据的单页 HTML (基于 Redoc)。

## 3. 优劣势分析

| 维度 | 优势 (Pros) | 劣势 (Cons) |
| :--- | :--- | :--- |
| **维护成本** | **极低**。开发者只写测试脚本，文档自动生成。 | 需要开发者遵守特定的注释规范，否则文档信息贫乏。 |
| **准确性** | **高**。文档基于实际可运行的 Request 生成。 | 静态分析难以处理高度动态的 Body（如通过 JS 脚本生成的 Body）。 |
| **可移植性** | **极强**。产物是标准 OpenAPI，可对接任何生态工具。 | 初始开发工作量较大（需要写完善的 Schema 推导逻辑）。 |
| **性能** | Rust 解析速度极快，秒级生成上千接口文档。 | 无。 |

## 4. 潜在风险与挑战 (Risks)

1.  **推导歧义 (Ambiguity)**:
    *   JSON `{"id": "123"}` 究竟是 String 还是 Int (in String)? 推导引擎只能猜 String。
    *   **对策**: 提供 `@type` 注释允许手动修正，例如 `// @type id: uuid`。

2.  **环境变量渲染**:
    *   URL 中包含 `{{baseUrl}}`。在文档中是显示 `{{baseUrl}}` 还是替换为 `localhost`?
    *   **对策**: `doc build` 命令支持传入 `--env dev`，生成文档时进行**静态替换**。

3.  **敏感信息泄露**:
    *   生成的文档可能包含 Header 中的 `Authorization: Bearer top-secret`（如果这是硬编码在 .http 里的）。
    *   **对策**: 文档生成器默认**自动过滤**常见的敏感 Header (Auth, Cookie)，除非显式白名单放行。

## 5. 与其他功能的联动

### 5.1 联动 AI (12. 接入 AI)
*   **AI 补全文档**: 如果开发者没写 `@desc`，可以调用 `rupost ai doc-gen`，让 AI 读取 Request 意图，自动生成描述并填入注释。
*   **Chat with Doc**: 生成的 `openapi.json` 可以喂给 LLM，实现“针对该 API 文档的问答助手”。

### 5.2 联动插件系统 (1. 插件系统)
*   文档格式扩展**: 插件可以注册新的 `DocExporter`。例如，有人想要导出为 `Postman Collection` 格式，可以通过插件实现，而不需要修改 RuPost 核心。

### 5.3 联动 CI/CD (8. CI/CD)
*   **自动发布**: 在 GitHub Actions 中，每次 Merge PR 自动运行 `rupost doc build`，并将 HTML 推送到 GitHub Pages。

## 6. 接口预留与扩展性设计

为了保证未来的扩展性，我们需要定义清晰的 **Trait 接口**：

```rust
/// 文档生成器适配器接口
pub trait DocGenerator {
    /// 接受 RuPost 的中间表示 (IR)，输出特定格式的文件内容
    fn generate(&self, collection: &RequestCollection) -> Result<Vec<GeneratedFile>>;
}

/// 默认实现：OpenAPI 生成器
pub struct OpenApiGenerator;
impl DocGenerator for OpenApiGenerator { ... }

/// 未来实现：Markdown 生成器
pub struct MarkdownGenerator;
impl DocGenerator for MarkdownGenerator { ... }
```

**配置预留 (`rupost.toml`)**:
```toml
[doc]
title = "My Project API"
version = "1.0.0"
output_format = ["openapi", "html"] # 支持多格式输出
exclude_files = ["private/*.http"]  # 排除不想公开的接口
```

## 7. 总结
本方案采用 **"Parse -> OpenAPI Model -> Render"** 的三阶段流水线。
核心难点在于 **类型推导** 和 **注释解析**。
一旦完成，将极大提升 RuPost 的商业价值和团队协作属性。建议作为 P1 级功能，紧随 AI 功能之后开发。
