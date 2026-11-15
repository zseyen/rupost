# RuPost 

> **极简、强大、富有美感的终端 HTTP 客户端与 API 测试工具**

RuPost 是一个基于 Rust 开发的现代化终端工具，旨在通过第一性原理重新定义 API 开发与测试体验。它不仅支持传统的 `.http` 文件，更能完美解析 Markdown 文件中的代码块，让文档即是测试，测试即是文档。

---

## 核心特性

- **极致美感**：精心设计的终端输出，采用 HSL 调色方案与平滑的微动画，追求极致的视觉交互体验。
- **文档即测试**：原生支持 `.http` 与 `.md` 文件解析。你可以直接在 Markdown 文档中编写和执行 API 请求。
- **强大断言**：内置高性能断言系统，支持 `@assert` 指令，轻松实现全自动接口验证。
- **AI 创作辅助**：`rupost ai assert` 用一句自然语言生成标准 `@assert` 断言行；支持任意 OpenAI 兼容后端（含本地 Ollama），密钥仅走环境变量，测试执行期零 AI 依赖。详见 [AI 断言生成指南](file:///examples/ai_assert/README.md)。
- **AI 失败智能处理**：`test --explain` 断言失败就地归因、`ai triage <report.json>` 批量失败聚类（E2/E5）；`ai resolve` 将 `# @ai-assert <自然语言>` 原地展开为标准断言；`ai bootstrap --from history` 把调试历史蒸馏成语义增强回归文件；`import curl` 把 curl 命令逆向为可执行 `.http` 请求块。详见 [AI 能力线文档](doc/plans/2026-10-28-ai-e2e5-master-execution-plan.md)。
- **变量与环境管理**：灵活的变量替换机制与多环境一键切换。详见 [变量与环境管理指南](file:///doc/guides/variables_and_environments.md)。
- **批量与并发测试**：支持跨文件有向无环图 (DAG) 拓扑排序依赖执行，支持顺序/并行双模式运行与状态单向克隆。详见 [批量测试指南](file:///doc/guides/batch_testing.md)。
- **网络诊断分析**：细粒度的网络耗时瀑布图与 X.509 证书到期健康分析。详见 [网络诊断工具指南](file:///doc/guides/network_diagnostics.md)。
- **全链路调试**：失败自动诊断、`--explain` 就地归因、`ai triage` 批量分诊、`--on-failure` trace 取证钩子、快照重放失败定位到条与历史导出供给链、**故障复现包**（`--incident-out`：失败运行产出脱敏、可重放、可移交的目录工件，仅失败时创建、绝不覆盖）。详见 [调试指南](file:///doc/guides/debug_guide.md)。
- **本地 Mock 服务**：Trie 树路径匹配、路径参数捕获与多分支条件响应。详见 [本地 Mock 服务器指南](file:///doc/guides/mock_server.md)。
- **WebSocket 协议调试**：支持 WebSocket 订阅流剧本、操作符软匹配断言与自动重连自愈。详见 [WebSocket 协议调试指南](file:///doc/guides/websocket_testing.md)。
- **历史与追踪**：自动记录请求历史，支持从历史记录交互式生成测试文件。
- **极致效率**：支持 curl 与 httpie 风格的命令行输入，并提供极短的别名（`t`, `h`, `g`）以提升操作流转速度。
- **交互式终端 TUI 模式**：内置基于 Model-View-Update 单向数据流与自适应布局的交互式调试终端。支持文件与请求历史双 Tab 状态解耦切换、基于内存预折行 (Pre-wrapping) 算法的长 Response/WS/SSE 日志高精度垂直滚动、路径显示配置切换、以及历史快照一键 Enter 反向还原代码并重新加载运行。
- **底层架构**：遵循 Clean Architecture 设计模式，代码结构清晰，易于扩展与维护。
- **插件生态**：hook（请求管线签名/认证/改写）与 command（CLI 工具扩展）双形态；命令型最小示例与作者指南见 [插件与扩展](file:///doc/architecture/features/plugin_and_extensibility.md)，社区插件索引见 [community-plugins.md](file:///doc/community-plugins.md)。

---

## 快速上手

### 安装

确保你已安装 Rust 环境，然后在项目目录下运行：

```bash
cargo build --release
```

### 常用命令大纲

RuPost 提供了直观且高效的命令行接口：

* **启动活文档工作台 (Web Workbench)**:
  ```bash
  rupost web
  # 指定端口与外部自定义前端静态目录 (覆盖内置前端)
  rupost web --port 8080 --static-dir ./custom_web_dist
  # 开启只读保护模式 (禁止在线保存覆盖文档)
  rupost web --readonly
  # 开发者跨域模式 (宽松 CORS 供独立前端联调；仅允许本地回环绑定，浏览器侧任意网页可读写本地文档，仅限临时调试)
  rupost web --dev
  ```
* **初始化环境模板与契约规格脚手架**:
  ```bash
  rupost init
  # 生成双角色活文档与 Mock 变体规格书
  rupost init spec -o spec.md
  ```
* **运行单个测试文件 (REST / WebSocket / SSE)**:
  ```bash
  rupost t examples/basic.http
  # 指定环境执行
  rupost t examples/basic.http -e dev
  ```
* **递归扫描并运行文件夹下所有测试**:
  ```bash
  rupost t examples/batch/
  # 开启并行模式与并发度控制
  rupost t examples/batch/ --mode parallel --concurrency 4
  ```
* **一键发起网络诊断**:
  ```bash
  rupost diagnose https://api.example.com
  ```
* **启动本地 Mock 服务器**:
  ```bash
  rupost mock examples/mock_config.json --port 9000
  ```
* **管理请求历史记录**:
  ```bash
  rupost history list --limit 10
  # 导出最近 N 条历史为可重放快照
  rupost history export --last 5 -o snap.json
  ```
* **快照重放与失败定位**:
  ```bash
  rupost replay snap.json --target http://staging.example.com
  # 失败输出 Failed entries 清单（定位到条），并以退出码 1 供 CI 门禁
  # body 深比较默认开启（timestamp/uuid 等动态键自动排除）；--explain 就地归因
  rupost replay snap.json -t http://staging.example.com --ignore-fields my_key --explain
  ```
* **Cookie 会话管理**:
  ```bash
  # 默认为临时会话：同一运行内的请求共享 Cookie，进程退出即失效
  rupost t login.http
  # 传入 --cookie-file 跨运行持久化（-e dev 会按环境隔离为 cookies_dev.json）
  rupost t login.http --cookie-file cookies.json -e dev
  rupost t whoami.http --cookie-file cookies.json -e dev
  # 完全禁用自动 Cookie
  rupost t api.http --no-cookies
  ```
* **SSE/LLM 模板快速生成**:
  ```bash
  rupost template sse -o my_sse.http
  ```
* **启动交互式终端 UI 调试模式 (TUI)**:
  ```bash
  rupost tui
  ```
* **导出测试用例为原生命令行 (cURL / HTTPie / Wget)**:
  ```bash
  # 查看支持的导出插件
  rupost export --list
  # 导出为 cURL 命令 (支持环境变量注入与凭据自动脱敏)
  rupost export curl examples/basic.http -e dev --redact
  # 导出单行紧凑格式并保存到脚本
  rupost export httpie examples/basic.http --compact -o request.sh
  ```

---

## 文件格式示例

### 1. `.http` 文件格式

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

**断言运算符**：`==`、`!=`、`contains`、`>`、`>=`、`<`、`<=`、`exists`（目标支持 `status`、`headers.*`、`body.<jsonpath>`、`response.time`）。
**动态变量**：`$uuid`、`$timestamp`、`$random_int` 可直接内插（如 `X-Request-Id: {{$uuid}}`）。
**超时**：`@timeout 500ms`（或 `1s`/`2m`）对非流式请求生效；SSE 用 `@sse_timeout`、WebSocket 握手用 `@timeout`。

### 2. `.md` 文件格式

可以直接在 Markdown 文档中书写测试代码块：

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
```

关于更高级的流式剧本（SSE/WebSocket），请参阅 [WebSocket 调试指南](file:///doc/guides/websocket_testing.md) 及 [SSE 测试指南](file:///doc/guides/sse_testing.md)。

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
2. **极致简洁**：奥卡姆剃刀，如无必要，勿增实体。
