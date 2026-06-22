# RuPost 大模型与 Server-Sent Events (SSE) 本地联调仿真示例

本目录提供了用于测试和联调大模型流式响应（LLM Streaming）与通用 Server-Sent Events (SSE) 接口的完整示例。

## 目录结构
*   `llm_demo.md`: 大模型流式测试用例与 Mock 契约定义。
*   `sse_demo.md`: 通用非大模型 SSE（构建日志流）测试用例与 Mock 契约定义。
*   `env.example`: 配套的环境变量配置文件。

---

## 🚀 极速上手：本地 100% 闭环流式联调

在没有真实 OpenAI / DeepSeek 等大模型 API Key 的情况下，您可以通过 RuPost 的 Mock 服务在本地无网闭环测试全链路的流式捕获与同步。

### 第一步：准备环境变量
将本目录下的 `env.example` 复制为根目录下的 `.env` 文件：
```bash
cp examples/llm_and_sse/env.example .env
```
此时 `.env` 中的 `BASE_URL` 会自动指向本地 Mock 服务：`http://127.0.0.1:8080/v1`。

### 第二步：启动本地仿真 Mock 网关
在新终端中，利用 `llm_demo.md` 中的 Mock 契约直接在 `8080` 端口上跑起模拟服务：
```bash
rupost mock examples/llm_and_sse/llm_demo.md --port 8080
```
Mock 服务启动后，会开始监听 `http://localhost:8080`，并已在内存中注册了用于大模型交互的 `/v1/chat/completions` 流式响应路由。

### 第三步：运行大模型流式测试
在另一个终端中，执行大模型的测试用例：
```bash
rupost test examples/llm_and_sse/llm_demo.md --verbose
```
**运行效果**：
1.  客户端向本地 Mock 发起 `POST /v1/chat/completions`。
2.  Mock 服务器识别出流式 `Content-Type: text/event-stream`，会自动以每帧 `40ms` 延迟的打字机流式效果输出模拟的 Token。
3.  客户端实时渲染并捕获流式数据。同时，用例二中的 `@stream_to` 会将接收到的 Token 增量同步写入本地物理文件 `./target/cloud_prompt_demo.md`。您可以使用分屏实时查看该文件的变化。

---

## 🛠️ 通用 SSE 行情/日志流联调

同样的，您也可以在本地闭环调试非大模型的通用推送事件流（如行情、日志）：

1.  启动通用 SSE 的 Mock 仿真：
    ```bash
    rupost mock examples/llm_and_sse/sse_demo.md --port 8080
    ```
2.  在另一终端执行测试：
    ```bash
    rupost test examples/llm_and_sse/sse_demo.md --verbose
    ```
    用例将对推送事件流中的 `stream.event` 类型（如 `build_start`）及 data 进行断言校验，并使用 `@stream_to` 将接收到的推送日志实时同步追加保存到 `./target/build_log_demo.txt`。
