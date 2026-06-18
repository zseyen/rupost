# Rupost Markdown 全生命周期 API 管理与解析规范

本规范旨在定义如何使用 `.md` 文件作为 API 的单一事实来源 (Single Source of Truth)，贯穿 API 设计、Mock 开发、自动化测试及持续迭代，并为 AI Agent 提供辅助决策所需的全局上下文 Memory。

---

## 🔍 1. 全生命周期下的 Markdown 功能定义

在 API 的生命周期中，Markdown 承载以下四种角色：

1. **API 设计 (Design)**: 顶部的 YAML Frontmatter 充当“全局元数据/知识库”，定义接口约束、安全等级、智能体规则（Agent Rules）等。
2. **API Mock (Mocking)**: 普通 HTTP 代码块代表接口的契约定义，包含多路条件响应变体（通过 `@mock-when` 区分分支），前端可调用本地 Mock 服务进行开发联调。
3. **API 测试与调试 (Testing & Debugging)**: 标有 `@test` 或 `# @test` 标记的代码块代表集成测试用例，包含具体的请求入参与 `@assert` 断言。
4. **API 迭代与 Memory (Evolution)**: 该 Markdown 文件作为 AI Agent 的 API 记忆体，使 Agent 具备架构的业务知识，确保新代码和测试始终符合契约规范。

---

## 📝 2. Markdown 编写规范

### 2.1 方案 A：混合单文件（全生命周期合一）
适合中小型 API，将设计、Mock 与集成测试统一维护在一个文件中。

```markdown
---
title: 用户中心 API
version: 1.0.0
base_path: /api/v1
security:
  - type: BearerAuth
    description: 所有的写操作及敏感查询均需在 Header 中携带 Authorization Token。
rules: |
  1. 所有 POST/PUT 请求的 id 必须是正整数。
  2. 敏感字段（如密码、身份证）必须进行脱敏或加密。
---

# 用户管理

## 获取用户详情
此接口用于查询用户的基本信息。支持不同角色的 Mock 变体。

```http
@name get-user-detail
GET /api/v1/users/:id

@mock-when query.role == admin
HTTP/1.1 200 OK
Content-Type: application/json

{
  "id": "{{id}}",
  "name": "管理员",
  "role": "admin"
}

@mock-when query.role == guest
HTTP/1.1 200 OK
Content-Type: application/json

{
  "id": "{{id}}",
  "name": "访客",
  "role": "guest"
}

@mock-default
HTTP/1.1 200 OK
Content-Type: application/json

{
  "id": "{{id}}",
  "name": "普通用户",
  "role": "user"
}
```

## @test 验证管理员获取详情流
此代码块标记为 `@test`，由测试运行器执行，不作为 Mock 变体。

```http
@name test-admin-detail
@test
GET http://localhost:9000/api/v1/users/123?role=admin
@assert status == 200
@assert body.role == admin
```
```

### 2.2 方案 B：多文件分离（设计与测试解耦）
当 API 测试用例规模庞大时，将设计契约与集成测试分离开来：
- `user_api.md`: 存放 Frontmatter 全局设计和所有的 Mock 契约定义。
- `user_test.md` (或 `user_test.http`): 专门存放集成测试用例，并在文件头部通过 `@depends-on user_api.md` 关联。

---

## ⚙️ 3. 代码解析与编译转换逻辑

### 3.1 提取全局知识 (YAML Frontmatter)
解析器在读取 `.md` 文件时，若头部以 `---` 包裹，则使用 YAML 解析器将其转换为全局元数据 Map，并挂载到解析产物 `ParsedFile` 中。

### 3.2 区分 Mock 契约与测试用例
- **测试块 (Test Blocks)**: 凡是含有 `@test` 指令的 HTTP 代码块，解析为 `ParsedRequest`，并在执行测试时正常运行；Mock 引擎启动时**忽略**测试块。
- **Mock 契约块 (Mock Blocks)**: 未含有 `@test` 的普通 HTTP 代码块。允许包含多个 `@mock-when` 以及 `@mock-default` 响应定义。
- **内存编译转换**: `rupost mock start <file.md>` 启动时：
  1. 调用 Markdown 解析器提取出所有的 Mock 契约块。
  2. 将每个契约块按照其路由（Method + Path）和各分支响应（变体）拆解、转换为内存中的 `MockRouteConfig` JSON 结构。
  3. 将转换后的 `Vec<MockRouteConfig>` 直接丢入 `AxumMockServer` 的模糊路径匹配引擎中运行。

---

## 🛠️ 4. 核心数据模型调整

### 4.1 引入 `ParsedMetadata` 与 `MockVariantConfig`
在 `src/parser/types.rs` 中：
```rust
use std::collections::HashMap;

/// 全局文件元数据（由 Frontmatter 解析而来）
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct FileMetadata {
    pub title: Option<String>,
    pub version: Option<String>,
    pub base_path: Option<String>,
    pub security: Vec<serde_json::Value>,
    pub rules: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// 升级的 ParsedFile 挂载元数据
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedFile {
    pub requests: Vec<ParsedRequest>,
    pub source_path: Option<PathBuf>,
    pub dependencies: Vec<String>,
    /// 新增：全局元数据与业务知识
    pub metadata: FileMetadata,
}
```

在 `src/parser/types.rs` 的 `ParsedRequest` 升级支持 Mock 响应变体：
```rust
/// Mock 变体在解析阶段的定义
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedMockVariant {
    /// 触发此变体的条件，例如 "query.role == admin"
    pub condition_expr: Option<String>,
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RequestMetadata {
    pub name: Option<String>,
    pub skip: bool,
    pub timeout: Option<Duration>,
    pub assertions: Vec<String>,
    pub captures: Vec<VariableCapture>,
    /// 新增：是否是测试用例
    pub is_test: bool,
    /// 新增：此请求所附带的 Mock 响应分支定义
    pub mock_variants: Vec<ParsedMockVariant>,
}
```

---

## 🧠 6. 与 Rupost 其他 Memory 方案的融合与对接

在 RuPost 的多维知识管理体系中，共有三种核心的 Memory（知识记忆）模式。本次 Markdown 生命周期规范与它们的结合点如下：

### 6.1 离线知识库 (Offline KB) 与 `Scenario` 规范对接
- **离线知识库中场景文件的解析复用**：在 `doc/plans/2026-04-16-offline-kb-design.md` 中，原子知识点存储在 `.rupost/kb/knowledge.jsonl`，而业务流程场景保存在 `.rupost/kb/scenarios/*.md`（带有 Frontmatter 和步骤引导）。
- **结合方式**：本次升级后的 `MarkdownFileParser` 具备强大的 Frontmatter 解析与 HTTP 块/元数据提取功能。离线知识库的场景描述文件可以直接复用该解析器进行无缝解析，将场景的前导信息、引导步骤及断言，统一映射到 `ParsedFile` 的 `metadata` 和 `ParsedRequest` 列表中，无需额外开发场景解析器。

### 6.2 历史请求快照库 (Snapshot/Replay Memory) 的双向流转
- **录制与生成 (Record -> Spec)**：通过 `rupost generate` 命令从 SQLite/JSON 历史快照库中提取特定的 Snapshot 时，系统将能够自动输出符合本次规范的 Markdown 请求块，并利用快照响应字段生成默认的 `@mock-default` 兜底响应。
- **重放与契约比对**：Mock 服务既支持直接重放 `SnapshotEntry`（回放模式），也支持通过 `MockCompiler` 将 Markdown 编译为 `MockRouteConfig`（契约模式）。

### 6.3 智能体上下文注入 (AI Agent Context Memory)
- **长效记忆注入**：当 AI Agent (如 `rupost-ai` 或外部协作 Agent) 执行测试生成、API 调试或自动化代码重构时，解析器提取的 `FileMetadata`（特别是其中的 `rules` 字段，如数据安全限制、类型约束）将作为 System Prompt 的 Context 强注入到大模型中，使得 Agent 的动作高度对齐业务设计规范。
- **无缝联调**：Agent 在演练或生成新接口测试时，可以通过配置的 Mock 条件快速验证逻辑，让 AI Agent 在闭环的“设计-Mock-测试”流程中拥有一个统一的、无需联网的本地“事实库 (Memory)”。

---

## 🔍 5. 编译转换方案 (Compiler Layer)

新增一个中间编译模块 `src/parser/converter.rs` (或在 `src/mock/converter.rs`)：
```rust
pub struct MockCompiler;

impl MockCompiler {
    /// 将解析后的 ParsedFile 转换为 Mock 引擎所接收的 MockRouteConfig 向量
    pub fn compile(parsed_file: &ParsedFile) -> Vec<MockRouteConfig> {
        let mut routes = Vec::new();
        // 过滤非测试块，只编译契约块
        for req in &parsed_file.requests {
            if req.metadata.is_test {
                continue;
            }
            
            // 构造 Mock 路由
            let path = req.url.clone(); // 例如 /api/v1/users/:id
            let method = req.method_or_default().to_string();
            
            let mut variants = Vec::new();
            for var in &req.metadata.mock_variants {
                // 解析 condition_expr 变成 VariantCondition
                let condition = var.condition_expr.as_ref().and_then(|expr| {
                    Self::parse_condition_expression(expr)
                });
                
                let mut headers = HashMap::new();
                for (k, v) in &var.headers {
                    headers.insert(k.clone(), v.clone());
                }
                
                variants.push(MockVariant {
                    condition,
                    status: var.status,
                    headers,
                    response_body: var.body.clone().unwrap_or_default(),
                });
            }
            
            // 如果 mock_variants 为空，则把请求的默认 Body 和期望响应（如果有的话）提取为兜底 Variant
            if variants.is_empty() {
                // 默认兜底
                variants.push(MockVariant {
                    condition: None,
                    status: 200,
                    headers: HashMap::new(),
                    response_body: String::new(),
                });
            }
            
            routes.push(MockRouteConfig {
                method,
                path,
                variants,
            });
        }
        routes
    }

    /// 将表达式解析为 VariantCondition
    /// 例如: "query.role == admin" -> Query, key="role", Equals, expected="admin"
    fn parse_condition_expression(expr: &str) -> Option<VariantCondition> {
        let parts: Vec<&str> = expr.split("==").map(|s| s.trim()).collect();
        if parts.len() != 2 {
            return None;
        }
        let lhs = parts[0];
        let expected_value = parts[1].trim_matches('"').to_string();
        
        let op = CompareOp::Equals;
        
        if lhs.starts_with("query.") {
            Some(VariantCondition {
                source: ConditionSource::Query,
                key: lhs["query.".len()..].to_string(),
                operator: op,
                expected_value,
            })
        } else if lhs.starts_with("header.") {
            Some(VariantCondition {
                source: ConditionSource::Header,
                key: lhs["header.".len()..].to_string(),
                operator: op,
                expected_value,
            })
        } else if lhs.starts_with("body.") {
            // body.$.user.role 等
            Some(VariantCondition {
                source: ConditionSource::Body,
                key: lhs["body.".len()..].to_string(),
                operator: op,
                expected_value,
            })
        } else {
            None
        }
    }
}
```
