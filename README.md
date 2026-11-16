# RuPost

> **极简、强大、富有美感的终端 API 活文档与回归测试引擎**

RuPost 是一个基于 Rust 构建的现代化 API 开发与测试工具。它主张**“文档即测试，测试即文档”**——不仅可以直接在命令行发送临时请求、解析传统 `.http` 文件，还能原生执行 Markdown（`.md`）文档中的请求代码块，并提供终端交互界面（TUI）与浏览器活文档工作台（Web）。

> 初次使用？推荐先阅读 **[5 分钟新手快速上手指南](doc/guides/quickstart.md)**。

---

## 一、三种工作模式，按需选择

| 模式 | 启动命令 | 适用场景 |
| :--- | :--- | :--- |
| **CLI 命令行与自动化** | `rupost GET ...` / `rupost test ...` | 临时发包、本地批量回归、CI/CD 门禁与契约校验 |
| **TUI 交互终端** | `rupost tui [file]` | 全键盘终端内编辑请求、查看时延瀑布、实时流式日志与一键 Mock |
| **Web 活文档工作台** | `rupost web` | 浏览器浏览 Markdown/HTTP 活文档、单块点击调试与团队只读分享 |

---

## 二、5 分钟快速开始

### 1. 安装

确保本机已安装 Rust 工具链（`rustc` / `cargo`），在仓库根目录执行：

```bash
# 安装到 ~/.cargo/bin（推荐）
cargo install --path .

# 或仅编译 release 二进制（target/release/rupost）
cargo build --release
```

### 2. 免建文件，直接发一个请求

```bash
# 支持 curl / httpie 风格的快捷请求
rupost GET https://httpbin.org/get
rupost POST https://httpbin.org/post name=alice role=admin

# 内联模式下，全局选项（如 --debug）需紧跟在 rupost 之后
rupost --debug GET https://httpbin.org/get
```

### 3. 创建或导入你的第一个测试文件

```bash
# 方式 A：生成初始化模板（不会覆盖已有文件）
rupost init

# 方式 B：直接把现有 curl 命令转换为可执行 .http 文件
rupost import curl 'curl -X POST https://api.example.com/login -d "{\"user\":\"alice\"}"' -o login.http
```

你也可以手写一个 `.http` 或 `.md` 文件（`demo.http`）：

```http
### 获取用户信息
# @name = GetUser
# @timeout = 3s
GET {{base_url}}/users/1
Authorization: Bearer {{token}}
X-Request-Id: {{$uuid}}

@assert status == 200
@assert body.name == "Alice"
@capture user_id from body.id
```

Markdown（`.md`）同样可以直接作为测试文件运行，只需把请求写在 ```` ```http ```` 代码块中：

````markdown
# 用户接口文档

```http
GET {{base_url}}/users/1
@assert status == 200
```
````

### 4. 运行测试并查看结果

```bash
# 运行单个文件（t 为 test 的别名）
rupost t demo.http

# 指定环境，并把提取到的 @capture 变量持久化供下次请求复用
rupost t demo.http -e dev --var-state

# 递归扫描并并行执行整个目录
rupost t examples/batch/ --mode parallel --concurrency 4
```

### 5. 进入可视化调试界面

```bash
# 启动终端交互界面（TUI）
rupost tui demo.http

# 启动浏览器活文档工作台（Web）
rupost web --port 8080
```

---

## 三、核心功能与高频命令速查

### 1. 文档测试、选择重跑与环境控制

- **单请求精选与依赖闭包**：用 `--only` 或 `#` 片段只跑某一条，配合 `--with-deps` 自动按 `@depends-on` 拓扑序先跑前置登录/准备文件：
  ```bash
  rupost t api.http#GetUser
  rupost t profile.http --only 1 --with-deps
  ```
- **变量与 Cookie 跨运行延续**：
  ```bash
  # 自动保存/回灌 @capture 变量（默认 .rupost/vars.json，显式 --var/.env/toml 恒优先）
  rupost t login.http --var-state
  # 持久化 Cookie（配合 -e dev 自动按环境隔离）
  rupost t login.http --cookie-file cookies.json -e dev
  ```
- **严格校验与无痕执行**：
  ```bash
  # 拦截未解析的 {{var}}、插件降级视为失败、且不写入本地历史文件
  rupost --no-history t suite.http --strict-vars --strict-plugins
  ```
- **内置断言与动态宏**：支持 `==`、`!=`、`contains`、`>`、`>=`、`<`、`<=`、`exists`（覆盖 `status`、`headers.*`、`body.<jsonpath>`、`response.time`），内置 `{{$uuid}}`、`{{$timestamp}}`、`{{$random_int}}`。

### 2. API 契约漂移检测、快照重放与调试排障

- **契约漂移基线（CI 门禁）**：首次运行生成字段结构基线，后续运行自动检出新增字段、类型变更与字段移除：
  ```bash
  rupost t api.http --contract-baseline baseline.json
  ```
- **网络诊断与失败归因**：
  ```bash
  # 分阶段检测 DNS / TCP / TLS / HTTP 耗时与证书健康度
  rupost diagnose https://api.example.com

  # 测试失败时保留脱敏、可重放的故障复现包（仅失败时创建，绝不覆盖已有目录）
  rupost t suite/ --incident-out ./incident-01 --explain
  ```
- **快照重放与本地 Mock**：
  ```bash
  # 录制并重放快照（支持自动改写目标地址并逐条定位响应体差异）
  rupost t suite.http --save-snapshot snap.json
  rupost replay snap.json --target http://staging.example.com --explain

  # 启动本地 Mock 服务器（支持 Trie 路径参数与多变体条件响应）
  rupost mock examples/incident_bundle/mock.json --port 9000
  ```

### 3. 多协议（WebSocket / SSE）与导入导出

- **WebSocket / SSE 流式测试**：支持 `@websocket` 长连接 `SEND`/`EXPECT` 剧本与自动重连、SSE 流式响应断言与大模型增量输出落盘（`@stream_to`）。
- **与 `curl` / `httpie` / `wget` 双向互通**：
  ```bash
  # 导入 curl 为 .http（可选将域名模板化为 {{base_url}}）
  rupost import curl 'curl https://api.example.com/items' --templatize-host -o items.http

  # 将 .http/.md 导出为脱敏命令行脚本
  rupost export curl demo.http -e dev --redact
  ```
- **历史管理与生成**：
  ```bash
  rupost history list --limit 10
  rupost history export --last 5 -o snap.json
  rupost generate --last 5 -o regression.http
  ```

### 4. AI 辅助创作与失败分诊（执行期零 AI 依赖）

支持任意 OpenAI 兼容接口及本地 Ollama；测试执行本身完全确定性，不依赖大模型：

```bash
# 一句自然语言生成标准 @assert 断言（可附带最近历史或响应文件上下文）
rupost ai assert "状态码为 200 且 data.items 非空" --last 1

# 将文件内的 `# @ai-assert <自然语言>` 原地展开为标准断言
rupost ai resolve demo.http --dry-run

# 从请求历史提炼语义增强回归测试（AI 不可用时自动降级为确定性基线）
rupost ai bootstrap --from history --last 5 -o regression.http

# 对 CI 失败 JSON 报告或故障快照做根因聚类分诊
rupost ai triage report.json
```

### 5. 插件与扩展生态

支持 `hook`（请求签名/认证/改写）与 `command`（CLI 扩展）双形态，具备静态 Manifest 预检与缺失自愈降级：

```bash
rupost plugin list
rupost plugin validate examples/plugins/rupost-plugin-cmd-hello.json
rupost plugin schema
```

---

## 四、工作区配置与凭据安全

RuPost 通过项目根目录 `rupost.toml` 管理环境、路径沙箱、插件默认门禁与 AI Profile，并通过 `$HOME/.rupost/credentials.toml`（强制 `0600` 权限与防符号链接保护）安全存放个人密钥：

```toml
# rupost.toml（可安全提交至 Git，禁止写入明文 api_key）
default_env = "dev"

[environments.dev]
base_url = "http://127.0.0.1:8080"

[paths]
history_file = ".rupost/history.jsonl"
logs_dir = ".rupost/logs"
snapshots_dir = "snapshots"

[plugins]
allow_missing_plugins = false
strict_plugins = false
strict_vars = false

[ai]
default_profile = "default"

[ai.profiles.default]
base_url = "https://api.openai.com/v1"
model = "gpt-4o-mini"
api_key_env = "OPENAI_API_KEY"
```

---

## 五、文档导航

| 主题 | 文档 |
| :--- | :--- |
| **新手入门** | [5 分钟快速上手指南](doc/guides/quickstart.md) · [用户手册](doc/guides/user_guide.md) |
| **环境与批量** | [变量与环境管理](doc/guides/variables_and_environments.md) · [批量与并发测试](doc/guides/batch_testing.md) |
| **调试与 Mock** | [全链路调试指南](doc/guides/debug_guide.md) · [网络诊断工具](doc/guides/network_diagnostics.md) · [本地 Mock 服务器](doc/guides/mock_server.md) |
| **流式协议** | [WebSocket 调试指南](doc/guides/websocket_testing.md) · [SSE 与大模型流式测试](doc/guides/sse_testing.md) · [LLM 模板](doc/guides/llm_templates.md) |
| **插件与生态** | [插件与扩展架构](doc/architecture/features/plugin_and_extensibility.md) · [社区插件索引](doc/community-plugins.md) |
| **项目演进** | [更新日志 (CHANGELOG)](CHANGELOG.md) · [进度汇总](doc/plans/progress_summary.md) |

---

## 六、技术栈与设计原则

- **技术选型**：Rust · Tokio · Clap · Reqwest (`rustls`) · Axum · Ratatui · Serde · Tracing
- **设计原则**：
  1. **第一性原理**：从零思考每一个功能的最优解。
  2. **Clean Architecture & 极致简洁**：严格分层解耦，奥卡姆剃刀，如无必要勿增实体。
