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

- **初始化环境配置模板**:
  ```bash
  rupost init
  ```

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

- **生成 SSE/LLM 调试模板**:
  ```bash
  rupost template sse -o my_sse.http
  # 或者自适应生成 Markdown 格式的模板
  rupost template sse -o my_sse.md
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
@capture auth_token from body.token
```

#### 通过命令行定义 (--var)
临时覆盖或新增变量：
```bash
rupost t test.http --var base_url=http://localhost:8080
```

#### 本地局部环境变量 (.env)
在本地开发联调时，如果不想修改共享的 `rupost.toml`（防止 Git 提交冲突），你可以在根目录下创建一个 `.env` 或 `.env.<env_name>` 文件：
```env
base_url = http://localhost:8081
api_key = my-local-key
```
RuPost 启动时会自动检测并加载 `.env`，从而对 `rupost.toml` 中的同名变量进行级联覆盖。

#### 全局共享变量 (@capture global. & env.)
- **全局变量**：在并发测试时若想跨文件安全共享数据，可在用例中捕获 `global.` 前缀变量，例如：
  ```http
  @capture global.token from body.token
  ```
  此时该变量会进入跨线程安全的全局共享区，其他并发运行的测试文件可通过 `{{global.token}}` 实时引用最新值。
- **系统环境直接映射**：在用例中可直接引用当前进程的环境变量（无需在 `rupost.toml` 中配置），例如：`{{env.USER}}`、`{{env.PATH}}`。

### 2. 级联覆盖优先级 (Cascading Priority)
在变量发生冲突时，RuPost 严格遵循以下优先级进行覆盖合并：
1. **最高优先级**：命令行 `--var` 传参 (如 `--var key=val`)
2. **第二优先级**：当前终端进程的**系统环境变量** (仅覆盖已有同名变量，不污染命名空间)
3. **第三优先级**：本地局部环境变量文件 (`.env` 或 `.env.<env_name>`)
4. **最低优先级**：共享 `rupost.toml` 环境配置中的默认变量

### 3. 使用变量

在 `.http` 或 `.md` 文件中，使用 `{{var_name}}` 语法引用变量：

```http
GET {{base_url}}/users/1
Authorization: Bearer {{token}}
```

### 4. 环境与局部变量文件切换

执行时可以通过 `-e` / `--env` 指定环境名，并可以通过 `--env-file` 显式指定局部变量文件的路径：

```bash
# 自动合并加载共享 dev 配置和本地局部默认 .env
rupost t examples/basic.http -e dev

# 显式指定加载本地特定的局部配置文件
rupost t examples/basic.http -e dev --env-file .env.staging
```

---

## 网络诊断工具 (Network Diagnostics)

为了帮助开发者在生产环境或本地开发时快速定位网络、TLS 握手及证书问题，RuPost 提供了一键式的网络诊断工具 `diagnose` (别名 `d`)：

```bash
rupost diagnose https://httpbin.org/get
# 或者使用别名
rupost d https://api.example.com
```

### 核心特性
- **细粒度时延瀑布图 (Waterfall)**：基于第一性原理手动连接握手，拆分并可视化输出 **DNS 解析、TCP 握手、TLS 协商及 HTTP TTFB** 的精确耗时表现。
- **X.509 证书深度健康分析**：展示证书的主体 SAN、颁发者、过期时间以及剩余天数，并针对过期（红色 `[EXPIRED]`）或临期（<= 30 天，黄色 `[WARNING]`）状态进行高亮预警。
- **网络层解耦**：采用独立子命令，与 HTTP 客户端、测试执行器高度隔离。

---

## WebSocket 协议测试与调试 (WebSocket Testing & Debugging)

RuPost 原生支持纯 WebSocket 协议的长连接测试、订阅与会话级调试。

### 1. 快速示例

在 `.http` 或 `.md` 代码块中，使用 `@websocket` 元数据指令标识 WebSocket 会话剧本：

```http
### 订阅 BTC 实时成交价
# @name WebSocket Basic Demo
# @websocket
# @assert body.price > 60000
# @capture btc_price from body.price
GET ws://localhost:8080/v1/market

# 1. 订阅动作发送 (默认发送 Outbound Text 帧)
SEND { "action": "subscribe", "topic": "ticker.btc" }

# 2. 阻塞式软匹配预期帧，支持 ==, !=, contains 运算符与变量引用
EXPECT $.event == "ticker"
@timeout = 3000

# 3. 混合匹配与包含运算符测试
EXPECT $.symbol contains "BT"
@timeout = 3000

# 4. 阻塞等待 500 毫秒后再进行下一步
WAIT 500

# 5. 主动优雅断开 WebSocket 连接
CLOSE
```

### 2. 剧本指令说明

* **`@websocket`**：声明当前请求块为 WebSocket 会话剧本。请求的方法必须是 `GET`，URL 协议必须是 `ws://` 或 `wss://`。
* **`SEND <payload>`**：向服务器发送一帧消息（默认为 Text 帧）。Payload 支持跨多行书写，也支持 JSON 结构的隐式发送。
* **`EXPECT <condition>`**：阻塞式等待入站帧，直到有帧能匹配上条件或等待超时（默认 5 秒超时，可用 `@timeout = <Duration>` 局部控制）。
  - **JSONPath 匹配**：例如 `EXPECT $.event == "ticker"`、`EXPECT $.price != 100` 或 `EXPECT $.message contains "hello"`。
  - **JSON 子集匹配**：例如 `EXPECT {"status": "ok"}`。
  - **文本子串匹配**：退化为模糊包含匹配（如 `EXPECT pong`）。
* **`WAIT <ms>`**：阻塞等待特定毫秒数。
* **`CLOSE`**：客户端主动优雅断开连接。

### 3. 高级特性

* **MessagePack 二进制解码**：通过在头部指定 `# @decoder messagepack`，客户端能自动将接收到的 Binary 帧通过 MessagePack 解码为 JSON 结构，使 `@assert` 和 `@capture` 对二进制消息同样完美生效。
* **重连自愈与强时序 Flush**：网络物理断开时，代理层后台会在约 30 秒内进行指数退避自动重连。重连期间发送的消息将暂存于 `pending_send_queue` 缓存区（限额 100 帧），重连成功后以 `push_front` 强时序 Flush 补发，保障消息时序零丢失。
* **滑动历史缓冲区**：历史帧缓冲区容量限制为 1000 帧，防止调试行情高频推送时内存膨胀（OOM）。

---

## 本地 Mock 服务器 (Mock Server)

RuPost 提供了一个独立且高可扩展的轻量级本地 Mock 服务器，让您能够依据接口定义或请求历史快照一键搭建本地 Mock 桩：

```bash
rupost mock examples/mock_config.json --port 9000
# 或者使用别名，并指定 Markdown 文档进行编译匹配
rupost m doc/plans/2026-06-06-mock-server-design.md
```

### 核心特性
- **两类数据输入支持 (Untagged Deserialize)**：
  1. **条件匹配规则集 (Routes)**：支持定义 HTTP 方法、Trie 路由与变体列表。
  2. **历史请求快照 (Snapshots)**：直接读入 RuPost 生成的历史导出快照包，自动转译为 Mock 规则。
- **Trie 树模糊路径匹配**：自主实现的 Trie 树路由匹配，支持精确路径、路径参数捕获（如 `/users/:id`，自动提取参数至 Context）和通配符匹配（`*`, `**`）。
- **多路分支条件匹配 (MockVariant)**：支持在同一路由下配置多个响应变体，按优先级匹配 Headers, Query 参数或 Body 内 JSONPath（支持 Equals/Contains/Exists 判定），并支持返回状态码、自定义响应头与动态变量渲染。
- **控制台高亮访问日志**：实时以高雅彩色打印请求的解析、路由命中及变体匹配情况。

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
@capture user_id from body.id
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
@capture token from body.token
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

