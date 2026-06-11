# RuPost SSE & 大模型 API 测试模板 (Markdown 格式)

此 Markdown 文件可以直接通过 `rupost test <本文件名>` 运行。
RuPost 会自动解析并执行其中包含在 ` ```http ` 语法块中的 API 请求。

## 场景一：零门槛本地闭环测试 (Local Mock)

首先，在终端启动本地模拟服务：`rupost mock --port 8080`。
随后运行本文件，即可直接观察 SSE 解析与断言。

```http
@sse
@assert status == 200
@assert stream.llm.content contains "Rust is perfect."
POST http://127.0.0.1:8080/v1/chat/completions
Content-Type: application/json

{
  "model": "mock",
  "messages": [{"role": "user", "content": "hello"}],
  "stream": true
}
```

## 场景二：真实大模型接口测试 (OpenAI / 兼容流式 API)

使用真实大模型接口前，请确保已经将同级目录下的 `.env.example` 文件重命名为 `.env` 并填写了真实的 `API_KEY`。

```http
@sse
@assert status == 200
@assert stream.llm.content contains "Rust"
@capture accumulated_reply from stream.llm.content
POST http://127.0.0.1:8080/v1/chat/completions
Content-Type: application/json
Authorization: Bearer {{env.API_KEY}}

{
  "model": "gpt-4",
  "messages": [{"role": "user", "content": "用两句话介绍 Rust 语言。"}],
  "stream": true
}
```

## 场景三：流式响应增量同步物理文件

增量追加或覆盖物理文件内容，适合一边流式输出一边用编辑器打开分屏实时阅读。

```http
@sse
@stream_to ./target/prompt_debug.md overwrite
POST http://127.0.0.1:8080/v1/chat/completions
Content-Type: application/json
Authorization: Bearer {{env.API_KEY}}

{
  "model": "gpt-4",
  "messages": [{"role": "user", "content": "请为我生成一份 Rust 基础教学大纲。"}],
  "stream": true
}
```

## 场景四：通用 non-LLM SSE 行情或日志流

系统构建日志流或推送流非 json 格式，会自动进入通用模式，并且依然能够通过 event 类型对指定帧做单独过滤断言。

```http
@sse
@assert stream.event == "build_start"
@assert stream.llm.content contains "Compiling"
POST http://127.0.0.1:8080/build-stream
```
