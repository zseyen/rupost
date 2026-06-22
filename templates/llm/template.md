---
base_path: /v1/chat/completions
---

# RuPost 大模型 (LLM) 接口测试与联调模板 (Markdown 格式)

本 Markdown 文件可以直接通过 `rupost test <本文件名>` 运行。
RuPost 会自动解析并执行其中包含在 ` ```http ` 语法块中的 API 请求。

## 使用与测试快速指引

1. 复制同目录下的环境变量模板并重命名为 `.env`：
   `cp .env.example .env`
2. 修改其中的 `API_KEY` 为您的大模型服务密钥，修改 `BASE_URL` 为模型服务的地址。
3. 运行测试：
   `rupost test <本文件名>`
4. 指定特定模型：
   本模板使用环境变量 `{{env.MODEL_NAME}}`。若要指定或对比不同的模型，可以直接在各个请求体 JSON 中将 `"model": "{{env.MODEL_NAME}}"` 修改为具体的模型名称（例如 `"model": "gpt-4o"`）。

---

## 场景一：本地大模型流式联调 (Local LLM - Ollama / vLLM)

```http
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

```http
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

`@capture` 自动将流式响应的拼接文本捕获为临时变量。

```http
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

以下块不带任何断言，当在本地使用 `rupost mock <本文件名>` 启动 Mock 服务时，它会被自动识别并编译为 Mock 路由。配合 Mock 引擎的流式模拟输出，能够在没有配置线上 API 密钥时，为接入大模型接口的客户端提供高度逼真的本地流式响应仿真。

```http
POST /v1/chat/completions
Content-Type: application/json

@mock-default
HTTP/1.1 200 OK
Content-Type: text/event-stream

data: {"choices":[{"delta":{"content":"Rust"}}]}

data: {"choices":[{"delta":{"content":" is"}}]}

data: {"choices":[{"delta":{"content":" perfect"}}]}

data: {"choices":[{"delta":{"content":"!"}}]}

data: [DONE]
```
