# RuPost 大模型 (LLM) 接口测试与联调示例

本 Markdown 文件可以直接通过 `rupost test <本文件名>` 运行。
RuPost 会自动解析并执行其中包含在 ` ```http ` 语法块中的 API 请求。

---

## 场景一：本地大模型流式联调 (Local LLM - Ollama / vLLM)

```http
# @test
# @sse
# @assert status == 200
# @assert stream.llm.content contains "Rust"
POST {{env.BASE_URL}}/chat/completions
Content-Type: application/json

{
  "model": "{{env.MODEL_NAME}}",
  "messages": [{"role": "user", "content": "请用一句话概括 Rust 语言的优势。"}],
  "stream": true
}
```

## 场景二：云端大模型 API 测试与 Token 物理同步 (Cloud LLM API)

`@stream_to` 将大模型流式响应的增量 Token 实时同步追加或覆写输出到指定的物理文件中。非常适合在 IDE 中使用分屏渲染功能实时阅读或配合生成报告。

```http
# @test
# @sse
# @assert status == 200
# @stream_to ./target/cloud_prompt_demo.md overwrite
POST {{env.BASE_URL}}/chat/completions
Content-Type: application/json
Authorization: Bearer {{env.API_KEY}}

{
  "model": "{{env.MODEL_NAME}}",
  "messages": [{"role": "user", "content": "请为我生成一份 Rust 基础教学大纲。"}],
  "stream": true
}
```

## 场景三：大模型令牌转接与网关代理 (Token Relay / Gateway Proxy)

`@capture` 自动将流式响应的拼接文本捕获为临时变量，可用于后续的请求链（例如将回复存入数据库或在后续请求中作为历史上下文传入）。

```http
# @test
# @sse
# @assert status == 200
# @capture gateway_reply from stream.llm.content
POST {{env.BASE_URL}}/chat/completions
Content-Type: application/json
Authorization: Bearer {{env.RELAY_TOKEN}}

{
  "model": "{{env.MODEL_NAME}}",
  "messages": [{"role": "user", "content": "你好。"}],
  "stream": true
}
```

---

## 场景四：本地大模型接口仿真 Mock 契约 (Mock Server Specification)

以下块不带任何 `@test` 断言，当在本地使用 `rupost mock <本文件名>` 启动 Mock 服务时，它会被自动识别并编译为 Mock 路由。配合 Mock 引擎的流式模拟输出，能够为客户端提供高度逼真的本地流式响应仿真。

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
