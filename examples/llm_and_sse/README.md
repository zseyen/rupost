# RuPost 大模型与 Server-Sent Events (SSE) 智能路由联调示例

本目录提供了用于联调大模型流式响应（LLM Streaming）与通用 Server-Sent Events (SSE) 接口的完整示例，展示了基于 `BASE_URL` 基础路径与可选 `base_path` 声明的自适应路由拼接机制。

## 目录结构
*   `llm_demo.md`: 大模型流式测试用例与 Mock 契约定义 (Markdown 格式)。
*   `llm_demo.http`: 大模型流式测试用例与相对路径演示 (HTTP 报文格式)。
*   `sse_demo.md`: 通用非大模型 SSE（构建日志流）测试用例与 Mock 契约定义。
*   `env.example`: 配套的环境变量配置文件。

---

## 核心特性：自适应路由智能拼接

为了让测试用例与大模型服务商（如 OpenAI、Ollama、Claude、DeepSeek）解耦，您无需在每个用例的 `POST` 请求行中硬编码完整的端点。Rupost 支持**基础路径 + 总体路径 + 用例路径**的自适应拼接机制：

1. **基础路径 (`BASE_URL`)**：在 `.env` 环境变量或环境配置（如 `rupost.toml`）中全局配置，作为 API 网关的基本路径。
   ```ini
   BASE_URL=https://api.deepseek.com
   ```
2. **总体路径 (`base_path`)**：文件级配置，用来可选地声明当前测试文件中所有用例公共的子路由路径：
   - 在 `.md` 文件顶部的 YAML Frontmatter 中声明：
     ```yaml
     ---
     base_path: /v1/chat/completions
     ---
     ```
   - 在 `.http` 文件顶部通过元数据指令可选配置：
     ```http
     # @base_path /v1/chat/completions
     ```
3. **用例路径**：用例块只写相对路径（如以 `/` 开头的子路由或空路径）：
   ```http
   POST /
   Content-Type: application/json
   ```
   Rupost 运行时会自动根据拼接公式合成最终请求 URL：
   `最终 URL = BASE_URL (去尾斜杠) + base_path (去首尾斜杠) + 请求用例路径 (去首斜杠)`
   *例如上例将自动合成为：`https://api.deepseek.com/v1/chat/completions`。*

---

## 🚀 极速上手：本地 100% 闭环流式联调

在没有真实大模型 API 密钥的情况下，您可以通过内置的 Mock 服务在本地无网测试全链路。

### 第一步：准备环境变量
将本目录下的 `env.example` 复制到项目根目录下并重命名为 `.env`：
```bash
cp examples/llm_and_sse/env.example .env
```
此时 `.env` 中的 `BASE_URL` 会自动指向本地 Mock 服务：`http://127.0.0.1:8080`（无需指定 `/v1/chat/completions`，路由由用例里的 `base_path` 自动拼接补全）。

### 第二步：启动本地仿真 Mock 网关
启动 `llm_demo.md` 中声明的 Mock 契约服务：
```bash
rupost mock examples/llm_and_sse/llm_demo.md --port 8080
```
Mock 服务启动后会开始监听 `http://localhost:8080`，并自动注册 `/v1/chat/completions` 路由。

### 第三步：运行自适应大模型流式测试
在另一个终端中，您可以选择执行 Markdown 或 HTTP 格式的测试用例：

*   **运行 Markdown 用例**：
    ```bash
    rupost test examples/llm_and_sse/llm_demo.md --verbose
    ```
*   **运行 HTTP 报文用例**：
    ```bash
    rupost test examples/llm_and_sse/llm_demo.http --verbose
    ```

**运行效果说明**：
1.  用例中的 `POST /` 将自动智能拼接出最终 URL `http://127.0.0.1:8080/v1/chat/completions` 并发向 Mock 端口。
2.  Mock 服务器收到请求后，会按照 `Content-Type: text/event-stream` 打字机效果，以每帧 `40ms` 延迟输出模拟 Token。
3.  客户端实时捕获流式数据，并通过内置的 LlmStream 适配器将增量文本收集，顺利通过流式断言（如 `stream.llm.content contains "Rust"`）。

---

## ☁️ 一键验证云端连接性 (Cloud Connection Verification)

由于默认的 `llm_demo.md` 中包含了“场景一（本地大模型）”和“场景三（中转网关代理）”等不需要或使用 mock 密钥的本地调试场景，当您将 `.env` 配置文件修改为真实的云端大模型密钥与地址并直接测试该文件时，那些本地场景在云端由于无授权访问会引发 401 报错。

为了能够一键、无干扰地验证云端大模型的连接性，我们专门提供了一个专用于云端真实联调的 `.http` 文件：
```bash
rupost test --env-file examples/llm_and_sse/.env examples/llm_and_sse/llm_cloud_test.http --verbose
```
**运行效果**：仅单独执行并验证云端大模型用例，过滤并避开其他本地环境用例，若您的 `API_KEY` 与模型配置有效，测试结果将直接以 100% 成功（绿勾）通过。

---

## 🛠️ 命令行参数覆盖基础路径 (CLI Override)
如果在云端 CI/CD 或特定测试环境里，您可以通过命令行参数动态地覆盖 `.env` 文件中的 `BASE_URL`，自动拼接出新路径下的完整端点，例如：
```bash
rupost test examples/llm_and_sse/llm_demo.md --var base_url=http://api-gateway.prod:9000 --verbose
```
执行器将自动以 `http://api-gateway.prod:9000` 作为基础路径进行路由拼接，这使得大模型测试用例具备极高的一致性、通用性与跨平台移植能力。
