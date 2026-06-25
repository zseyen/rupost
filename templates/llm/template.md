---
base_path: /v1/chat/completions
---

# RuPost 大模型 (LLM) 接口测试与联调模板 (Markdown)

本 Markdown 文件可以直接通过 `rupost test <本文件名>` 运行，也可以使用 `rupost mock <本文件名>` 启动 Mock 仿真。

提示：如果需要测试或仿真非 JSON 格式的通用事件推送（如构建日志流、实时状态推送），请参考通用 SSE 模板：
[templates/sse/template.md](file:///Users/zsyzzx/.gemini/antigravity/worktrees/rupost/sse-debug-test-first/templates/sse/template.md)

## 极速上手配置

1. 复制同目录下的环境变量模板并重命名为 `.env`：
   `cp .env.example .env`
2. 修改其中的 `API_KEY` 为您的大模型服务密钥，修改 `BASE_URL` 为模型服务的基准地址。
3. 执行测试：
   `rupost test <本文件名>`
4. 启动本地 Mock 仿真（无需真实 API 密钥与网络连接）：
   `rupost mock <本文件名> --port 8080`

---

## 场景一：本地大模型流式联调 (Local LLM - Ollama / vLLM)

使用 # @sse 声明请求为 SSE 流式响应，RuPost 将自动按 SSE 协议解析流式帧并合成最终大模型文本内容。

```http
# @test
# @sse
# @assert status == 200
# @assert stream.llm.content contains "Rust"
POST /
Content-Type: application/json

{
  "model": "{{env.MODEL_NAME}}",
  "messages": [{"role": "user", "content": "请用一句话概括 Rust 语言的优势。"}],
  "stream": true
}
```

## 场景二：云端大模型 API 测试 (Cloud LLM API)

使用真实的大模型 API 进行连接并断言测试，可通过 {{env.API_KEY}} 自动注入密钥。

```http
# @test
# @sse
# @assert status == 200
POST /
Content-Type: application/json
Authorization: Bearer {{env.API_KEY}}

{
  "model": "{{env.MODEL_NAME}}",
  "messages": [{"role": "user", "content": "请为我生成一份 Rust 核心特性教学大纲。"}],
  "stream": true
}
```

## 场景三：大模型令牌转接与网关代理 (Token Relay / Gateway Proxy)

通过 @capture 语法将流式响应的打字机拼接结果在流式结束后捕获为临时变量，可用于后续的请求链中作为引用。

```http
# @test
# @sse
# @assert status == 200
# @capture gateway_reply from stream.llm.content
POST /
Content-Type: application/json
Authorization: Bearer {{env.RELAY_TOKEN}}

{
  "model": "{{env.MODEL_NAME}}",
  "messages": [{"role": "user", "content": "你好，请打个招呼。"}],
  "stream": true
}
```

---

## 场景四：本地大模型接口仿真 Mock 契约 (Mock Server Specification)

以下块不带任何断言或测试指令（未标明 @test），当运行 `rupost mock <本文件名>` 时会被自动注册为本地 Mock 路由。
这为需要大模型打字机流式响应的客户端应用提供逼真的本地仿真，支持在无网、无 API_KEY 状态下进行客户端与网关的联调。

```http
POST /v1/chat/completions
Content-Type: application/json

@mock-default
HTTP/1.1 200 OK
Content-Type: text/event-stream
Cache-Control: no-cache
Connection: keep-alive

data: {"choices":[{"delta":{"content":"Rust"}}]}

data: {"choices":[{"delta":{"content":" is"}}]}

data: {"choices":[{"delta":{"content":" perfect"}}]}

data: {"choices":[{"delta":{"content":"!"}}]}

data: [DONE]
```
