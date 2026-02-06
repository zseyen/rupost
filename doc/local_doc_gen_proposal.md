# 本地文档生成 (Local Document Generation) 详解方案

## 1. 核心概念
**“代码即文档” (Docs as Code)**。
用户不需要额外维护一份 API 文档（如 Swagger/Word/Wiki），RuPost 直接读取项目中的 `.http` 请求文件，提取其中的元数据、注释和请求结构，编译生成一套美观、可交互的静态 HTML 网站或 Markdown 文档。

**核心价值**：
1.  **单一数据源 (Single Source of Truth)**：API 定义变了，文档自动变，永远不会过时。
2.  **零成本交付**：后端开发写好测试脚本，运行一条命令，就能把文档发给前端。
3.  **私有化部署**：生成的 HTML 是纯静态文件，可以部署在内网 Nginx，完全不依赖外网 SaaS。

## 2. 功能设计

### 2.1 用户交互 (CLI)
增加 `doc` 子命令：

```bash
# 扫描当前目录，生成 HTML 文档到 ./dist 目录
rupost doc build --out ./dist --title "My API Docs"

# 启动一个本地服务器预览文档 (支持热重载)
rupost doc serve --port 3000
```

### 2.2 文档内容来源
RuPost 需要解析 `.http` 文件中的特定注释作为文档内容。

**示例 `.http` 文件**:
```http
### @name GetUserProfile
### @desc 获取用户详细信息，包含积分和等级。
### @tag 用户模块,只读
GET https://api.example.com/users/{{userId}}
Authorization: Bearer {{token}}

# 响应示例 (可选，RuPost 可以把最近一次测试的 Saved Response 嵌入文档)
```

**生成的文档包含**:
*   **左侧导航栏**: 按文件夹或 `@tag` 分组的 API 列表。
*   **中间主区域**:
    *   API 名称、描述。
    *   请求方法、URL、Headers、Query 参数表格。
    *   Body 结构说明（支持从 JSON Body 推导 Schema）。
*   **右侧/底部示例区**:
    *   请求代码示例 (Curl, Python, JS)。
    *   响应示例 (JSON 高亮)。

## 3. 技术实现方案

### 3.1 架构流程

```mermaid
graph LR
    Source[.http Files] -->Parser[RuPost Parser]
    Parser -->|AST| DocGenerator[Doc Generator]
    
    subgraph Generator
        DocGenerator -->|Extract| MetaData[Tags/Desc/Name]
        DocGenerator -->|Infer| Schema[JSON Schema]
        DocGenerator -->|Load| Template[HTML Templates (Tera/MiniJinja)]
    end
    
    Template -->|Render| HTML[Static HTML/JS/CSS]
    HTML -->|Deploy| Browser
```

### 3.2 关键技术点

1.  **元数据增强**:
    *   扩展 parser，支持解析 `### @desc` 多行描述（支持 Markdown 语法）。
    *   支持 `### @param id: 用户ID` 这样的参数说明注释。

2.  **类型推导 (Type Inference)**:
    *   RuPost 分析 Request/Response JSON Body，自动反推字段类型。
    *   例如 `{"age": 18}` -> 文档显示 `age: Integer`。

3.  **单页应用 (SPA) 模板**:
    *   不建议生成多页 HTML。建议生成一个 `index.html` + `doc_data.json`。
    *   前端模板可以使用 React/Vue 编写并打包成纯静态资源，RuPost 运行时只需嵌入这些静态资源。
    *   **推荐方案**: 复用现有的开源文档 UI 库（如 `Redoc` 或 `RapiDoc`），RuPost 只负责生成符合 OpenAPI (Swagger) 规范的 `swagger.json`，然后直接嵌入 Redoc 的 HTML 模板中。

### 3.3 方案对比：自研 UI vs 转 OpenAPI

| 方案 | 描述 | 优势 | 劣势 | 推荐 |
| :--- | :--- | :--- | :--- | :--- |
| **方案 A: 自研 HTML 模板** | 使用 Rust 模板引擎渲染 HTML | 完全可控，轻量级，无 JS 依赖 | 开发工作量大，UI 很难做得专业 | 否 |
| **方案 B: 生成 OpenAPI Spec** | 将 `.http` 转为 `openapi.json` | 生态极其丰富，可直接对接 Redoc/SwaggerUI | `.http` 的表达能力弱于 OpenAPI，转换可能有损耗 | **是** |

## 4. 推荐实施路径 (Adopt OpenAPI Path)

鉴于从零写一个漂亮的文档 UI 成本极高，最聪明的做法是 **RuPost 扮演 Converter 角色**。

1.  **第一步**: 实现 `rupost doc export`。
    *   读取所有 `.http` 请求。
    *   转换为标准 `OpenAPI 3.1 (Swagger)` JSON 格式。
2.  **第二步**: 集成 Swagger UI / Redoc。
    *   `rupost doc serve` 实际上就是启动一个微型 HTTP Server，提供 `openapi.json` 和一个嵌入了 Redoc 的 `index.html`。

这样做，你立刻就拥有了世界级的文档展示效果，而只需专注于解析 `.http` 文件。
