# RuPost 

> **极简、强大、富有美感的终端 HTTP 客服端与 API 测试工具**

RuPost 是一个基于 Rust 开发的现代化终端工具，旨在通过第一性原理重新定义 API 开发与测试体验。它不仅支持传统的 `.http` 文件，更能完美解析 Markdown 文件中的代码块，让文档即是测试，测试即是文档。

---

## 核心特性

- **极致美感**：精心设计的终端输出，采用 HSL 调色方案与平滑的微动画，追求极致的视觉交互体验。
- **文档即测试**：原生支持 `.http` 与 `.md` 文件解析。你可以直接在 Markdown 文档中编写和执行 API 请求。
- **强大断言**：内置高性能断言系统，支持 `@assert` 指令，轻松实现全自动接口验证。
- **变量与环境管理**：灵活的变量替换机制与多环境（dev, staging, prod）一键切换。
- **历史与追踪**：自动记录请求历史，支持从历史记录交互式生成测试文件。
- **极致效率**：支持 curl 与 httpie 风格的命令行输入，并提供极短的别名（`t`, `h`, `g`）以提升操作流转速度。
- **底层架构**：遵循 Clean Architecture 设计模式，代码结构清晰，易于扩展与维护。

---

## 快速上手

### 安装

确保你已安装 Rust 环境，然后在项目目录下运行：

```bash
cargo build --release
```

### 基础用法

RuPost 提供了直观的命令行界面：

- **运行测试文件**:
  ```bash
  rupost t examples/basic.http
  # 或者使用完整命令
  rupost test examples/basic.http
  ```

- **查看请求历史**:
  ```bash
  rupost h l --limit 10
  # 或者
  rupost history list
  ```

- **生成测试脚本**:
  ```bash
  rupost g new_test.http --interactive
  ```

- **类 curl 调用**:
  ```bash
  rupost GET http://httpbin.org/get -H "Authorization: Bearer token"
  ```

- **批量与目录递归执行**:
  ```bash
  rupost test examples/batch/
  # 或者运行特定的多个文件并指定并行模式
  rupost t examples/batch/03_health.http examples/batch/01_login.http --mode parallel --concurrency 2
  ```

---

## 批量测试与目录递归执行 (Batch & Directory Testing)

RuPost 支持对整个目录或多个文件进行批量测试。内置了依赖图解析与高效的执行模式：

### 1. 递归目录与隐藏路径过滤
当传入一个或多个文件夹时，RuPost 会自动递归扫描并收集所有的 `.http` 与 `.md` 文件，同时自动过滤并跳过 `.git/`、`.rupost/`、`target/` 和 `node_modules/` 等隐藏与无关路径。

### 2. 声明式跨文件依赖 (@depends-on)
支持在 `.http` 或 `.md` 文件的头部通过 `### @depends-on <filename>` 声明前置依赖关系。例如：
```http
### @depends-on 01_login.http
GET {{base_url}}/profile
Authorization: Bearer {{my_token}}
```
RuPost 会构建有向无环图（DAG），利用 **Kahn 拓扑排序算法** 自动计算并编排正确的用例执行顺序（如果检测到循环依赖将报错退出）。

### 3. 双执行模式选择
- **顺序模式 (Sequential)**：默认行为。按拓扑排序后的链式顺序执行，用例之间**共享变量上下文与 Cookie 会话**。
- **并行模式 (Parallel)**：通过 `--mode parallel` 开启，支持通过 `--concurrency <N>` 设置并发数。在有依赖关系的节点间，通过 **DAG 状态单向克隆（State Cloning）** 传递局部变量增量与 Cookie 状态，确保依赖节点间能完美传递鉴权 Token 并继承 Cookie 状态；而对于不相关的并行分支依然保持完全无锁的隔离，防范并发竞争。

### 4. 安全沙箱加载防护 (Sandbox Scope Jail)
在递归自动补全加载声明的依赖文件时，RuPost 会强制校验依赖的绝对物理路径，确保其只能位于当前工作目录（CWD）或用户指定的执行目录下。非本沙箱范围的文件或非支持测试扩展名（`.http`/`.md`）的文件一律拒绝加载，彻底防范路径穿越（Path Traversal）安全漏洞。

### 5. 高级输出控制
- **早期中断**：使用 `--fail-fast` 可在遇到第一个用例文件失败时，立刻终止后续测试。
- **结构化 JSON 报告**：使用 `--report json` 会将批量测试结果输出为标准 JSON，完美兼容 CI/CD 自动化分析。

---

## 变量与环境 (Variables & Environments)

RuPost 提供了一套灵活且强大的变量系统，遵循“一次配置，多处复用”的原则。

### 1. 定义变量

#### 在 `rupost.toml` 中定义 (推荐)
在项目根目录创建或编辑 `rupost.toml`，按环境组织变量：

```toml
[environments.dev]
base_url = "https://httpbin.org"
token = "dev-token-123"

[environments.prod]
base_url = "https://api.example.com"
token = "${PROD_TOKEN}" # 支持引用系统环境变量
```

#### 从响应中动态捕获 (@capture)
在请求块中使用 `@capture` 从上一个响应中提取数据：

```http
POST /login
# ...
@capture auth_token = body.token
```

#### 通过命令行定义 (--var)
临时覆盖或新增变量：
```bash
rupost t test.http --var base_url=http://localhost:8080
```

### 2. 使用变量

在 `.http` 或 `.md` 文件中，使用 `{{var_name}}` 语法引用变量：

```http
GET {{base_url}}/users/1
Authorization: Bearer {{token}}
```

### 3. 环境切换

执行时通过 `-e` 或 `--env` 指定环境：

```bash
rupost t examples/basic.http -e dev
```

---

## 文件格式示例

### `.http` 文件

```http
### 获取用户信息
@name = GetUser
@timeout = 3000
GET {{baseUrl}}/users/1
Authorization: Bearer {{token}}

@assert status == 200
@assert body.name == "Alice"
@capture user_id = body.id
```

### `.md` 文件

直接在 Markdown 中编写测试：

```markdown
# 登录接口测试

使用下面的代码块执行登录：

```http
POST /login
Content-Type: application/json

{
  "username": "admin",
  "password": "{{password}}"
}

@assert status == 200
@capture token = body.token
```

---

## 技术选型

- **Language**: Rust
- **Async Runtime**: Tokio
- **CLI Framework**: Clap
- **HTTP Client**: Reqwest
- **Serialization**: Serde
- **Logging**: Tracing

---

## 贡献与设计原则

RuPost 始终坚持：
1. **第一性原理**：从零思考每一个功能的最优解。
2. **极致简洁**：奥卡姆剃刀，如无必要，勿增实体

---

