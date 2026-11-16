# RuPost 新手快速上手指南

本指南面向初次接触 RuPost 的开发者，带你在 **5~10 分钟**内从“发送第一个请求”一路走到“编写自动化测试、切换环境、使用 TUI/Web 调试界面与配置工作区”。

---

## 1. 安装与验证

确保本地已安装 Rust 工具链（`rustc` / `cargo`），在项目根目录执行：

```bash
# 编译并安装到 ~/.cargo/bin（推荐，后续可全局直接运行 rupost）
cargo install --path .

# 或者仅编译 release 二进制（位于 target/release/rupost）
cargo build --release
```

安装完成后检查版本与命令列表：

```bash
rupost --version
rupost --help
```

---

## 2. 第一步：不建文件，直接发请求

像 `httpie` 或 `curl` 一样，你可以直接把 HTTP 方法和 URL 传给 `rupost`：

```bash
# 发送 GET 请求
rupost GET https://httpbin.org/get

# 发送 POST JSON 请求（key=value 语法自动组装 JSON）
rupost POST https://httpbin.org/post name=alice role=admin

# 附带自定义 Header
rupost GET https://httpbin.org/headers X-Demo:hello
```

> **提示**：内联请求模式下，全局选项（如 `--debug`、`--no-history`）必须紧跟在 `rupost` 后面，例如 `rupost --debug GET https://httpbin.org/get`。

---

## 3. 第二步：把常用请求写成 `.http` 或 `.md` 文件

### 3.1 方式一：用脚手架生成模板

```bash
# 生成基础配置与示例文件（不会覆盖已存在文件）
rupost init

# 生成 SSE 流式测试模板
rupost init sse -o sse_demo.http
```

### 3.2 方式二：从现有 `curl` 命令一键导入

如果你手头已经有从浏览器 DevTools 复制的 `curl` 命令，可以直接转成 `.http`：

```bash
rupost import curl 'curl -X POST https://api.example.com/login -H "Content-Type: application/json" -d "{\"user\":\"alice\"}"' -o login.http

# 加上 --templatize-host 会自动把域名替换为 {{base_url}}，方便切环境
rupost import curl 'curl https://api.example.com/users' --templatize-host -o users.http
```

### 3.3 手写 `.http` 与 `.md`（文档即测试）

**`.http` 示例（`demo.http`）**：

```http
### 登录并提取 token
# @name = Login
# @timeout = 3s
POST {{base_url}}/login
Content-Type: application/json

{
  "username": "alice",
  "password": "{{password}}"
}

@assert status == 200
@assert body.token exists
@capture token from body.token

### 使用上一步提取的 token 查询用户信息
# @name = GetProfile
GET {{base_url}}/me
Authorization: Bearer {{token}}
X-Request-Id: {{$uuid}}

@assert status == 200
@assert body.username == "alice"
```

**`.md` 示例（`api-doc.md`）**：直接把请求放在 Markdown 的 ```` ```http ```` 代码块中，文档和测试用例合二为一：

````markdown
# 用户接口文档

## 获取用户列表

```http
GET {{base_url}}/users
@assert status == 200
```
````

运行测试文件（`t` 是 `test` 的简写）：

```bash
rupost t demo.http
rupost t api-doc.md
```

---

## 4. 第三步：多环境、变量持久化与局部重跑

### 4.1 用 `rupost.toml` 管理环境

在项目根目录创建 `rupost.toml`：

```toml
default_env = "dev"

[environments.dev]
base_url = "http://127.0.0.1:8080"

[environments.staging]
base_url = "https://staging.example.com"
```

通过 `-e` / `--env` 切换环境，或用 `--var` 临时覆盖单个变量：

```bash
rupost t demo.http -e staging
rupost t demo.http --var base_url=http://127.0.0.1:9000
```

### 4.2 只跑某一个请求 / 带上前置依赖

当一个测试文件有很多请求、或者依赖其他文件中的登录请求时：

```bash
# 只运行第 2 个请求（或按 @name 指定：demo.http#GetProfile）
rupost t demo.http --only 2
rupost t demo.http#GetProfile

# 若目标文件通过 @depends-on 依赖 login.http，加上 --with-deps 自动先跑依赖
rupost t profile.http --only 1 --with-deps
```

### 4.3 跨运行保留变量与 Cookie

```bash
# 自动保存与回灌 @capture 捕获的变量（默认写入 .rupost/vars.json）
rupost t login.http --var-state
rupost t profile.http --var-state

# 跨运行持久化 Cookie（配合 -e 自动按环境隔离）
rupost t login.http --cookie-file cookies.json -e dev
```

---

## 5. 第四步：使用交互式界面（TUI 与 Web 工作台）

### 5.1 终端交互界面（TUI）

适合习惯全键盘在终端里边改边测的开发者：

```bash
rupost tui
# 也可以直接打开指定文件
rupost tui demo.http
```

常用操作速览：
- `Tab` / `Shift+Tab`：在文件树、编辑器、响应面板之间切换焦点
- `Enter` / `r`：发送当前请求或运行文件
- `#`：输入序号或 `@name` 选择文件中的指定请求
- `/`：过滤侧边栏文件
- `M`：一键启停当前文件对应的本地 Mock 服务
- `Ctrl+S`：保存脱敏后的运行快照

### 5.2 浏览器活文档工作台（Web Workbench）

适合团队协作阅读 Markdown/HTTP 文档，并直接在页面里点选代码块调试：

```bash
# 启动并绑定本地地址（默认端口可由 --port 指定）
rupost web --port 8080

# 只读分享模式（禁止通过网页修改磁盘文档与发起写执行）
rupost web --readonly
```

> 每个浏览器会话通过 `rupost_session` Cookie 自动隔离变量与 Cookie 状态，多人同时访问也不会互相串号。

---

## 6. 第五步：出错排查、契约检测与 CI 门禁

1. **网络连通性与证书检查**：
   ```bash
   rupost diagnose https://api.example.com
   ```
2. **失败时保留完整复现现场**：
   ```bash
   # 仅在测试失败时生成脱敏、可重放的故障复现包（目标目录不可已存在）
   rupost t suite/ --incident-out ./incident-01 --explain
   ```
3. **重放快照对比差异**：
   ```bash
   rupost replay ./incident-01/snapshot.json --target http://127.0.0.1:8080 --explain
   ```
4. **API 契约漂移检测（适合接入 CI）**：
   ```bash
   # 首次运行建立结构基线；后续运行自动对比字段新增、类型变更或字段删除
   rupost t api.http --contract-baseline baseline.json
   ```
5. **严格模式与无痕运行**：
   ```bash
   # 未解析的 {{var}} 立即报错拦截；插件降级视为失败；不写入本地 history.jsonl
   rupost --no-history t api.http --strict-vars --strict-plugins
   ```

---

## 7. 进阶配置：`rupost.toml` 与凭据安全

RuPost 推荐**“项目配置与个人密钥分离”**：
- 项目内放 `rupost.toml`（可提交 Git，**切勿写入明文 API Key**）：
  ```toml
  [paths]
  # 所有路径均严格限制在当前工作区目录内，防止越界
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
- 个人敏感密钥放在用户主目录 `~/.rupost/credentials.toml`（权限须为 `0600`）：
  ```toml
  [ai.profiles.default]
  api_key = "sk-..."
  ```

配置好环境变量或 `credentials.toml`（或本机运行 Ollama）后，即可体验 AI 辅助命令：

```bash
# 用自然语言生成标准 @assert 断言
rupost ai assert "状态码 200 且 data.items 非空"

# 将文件中的 `# @ai-assert ...` 占位符原地展开为正式断言
rupost ai resolve demo.http --dry-run

# 从最近请求历史提炼生成回归测试文件
rupost ai bootstrap --from history --last 5 -o regression.http
```

---

## 8. 下一步阅读

- [用户手册](user_guide.md) — 更完整的命令与功能说明
- [变量与环境管理](variables_and_environments.md) — 多层变量覆盖顺序详解
- [批量测试指南](batch_testing.md) — 多文件目录扫描、DAG 依赖编排与并发控制
- [调试指南](debug_guide.md) — 失败定位、快照重放与复现包工作流
- [本地 Mock 服务器](mock_server.md) — 离线联调与条件变体响应
- [WebSocket 测试指南](websocket_testing.md) / [SSE 测试指南](sse_testing.md) — 长连接与大模型流式调试
