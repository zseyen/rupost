# RuPost 通用 Server-Sent Events (SSE) 接口测试模板 (Markdown)

本 Markdown 文件可以直接通过 `rupost test <本文件名>` 运行，也可以使用 `rupost mock <本文件名>` 启动 Mock 仿真。

提示：如果需要测试或仿真标准大模型（如 OpenAI、DeepSeek 等 completions 规范）接口，请参考大模型专用模板：
[templates/llm/template.md](file:///Users/zsyzzx/.gemini/antigravity/worktrees/rupost/sse-debug-test-first/templates/llm/template.md)

## 极速上手配置

1. 启动本地 Mock 仿真服务：
   `rupost mock <本文件名> --port 8080`
2. 在新终端运行本文件以触发流式请求及断言测试：
   `rupost test <本文件名>`

---

## 场景一：零门槛本地闭环测试 (Local Generic SSE Mock)

使用 # @sse 声明请求为 SSE 事件流。由于是通用 SSE 流，支持使用：
- stream.event：断言当前事件名 (对应 SSE 的 event 字段)
- stream.body.<path>：断言当前事件的 JSON 负载字段 (对应 SSE 的 data 字段)

因为通用 SSE 每帧都会触发断言，我们可以使用 # @sse_max_events 1 指定仅接收和评估第一帧，以防止后续事件帧发生断言冲突。

```http
# @test
# @sse
# @sse_max_events 1
# @assert status == 200
# @assert stream.event == "build_start"
# @assert stream.body.status contains "Compiling"
POST http://127.0.0.1:8080/build-stream
Content-Type: application/json

{
  "stream": true
}
```

## 场景二：流式响应实时增量保存到物理文件

通过 @stream_to 将接收到的 SSE 推送日志实时追加 (append) 或覆盖 (overwrite) 写入本地物理文件中。

```http
# @test
# @sse
# @sse_max_events 1
# @stream_to ./target/build_log.txt overwrite
POST http://127.0.0.1:8080/build-stream
Content-Type: application/json

{
  "stream": true
}
```

---

## 场景三：本地通用 SSE 接口仿真 Mock 契约 (Mock Server Specification)

以下块不带任何测试指令（未标明 @test），当运行 `rupost mock <本文件名>` 时会被自动注册为本地 Mock 路由。
配合 Mock 引擎的流式模拟输出，能够为接入通用 SSE 接口的客户端提供高度逼真的本地流式响应仿真。

```http
POST /build-stream
Content-Type: application/json

@mock-default
HTTP/1.1 200 OK
Content-Type: text/event-stream
Cache-Control: no-cache
Connection: keep-alive

event: build_start
data: {"status": "Compiling rupost v0.2.0..."}

event: build_progress
data: {"status": "Compiling crates/rupost-ai v0.2.0..."}

event: build_complete
data: {"status": "Finished build!"}
```
