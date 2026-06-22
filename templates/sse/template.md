# RuPost 通用 Server-Sent Events (SSE) 接口测试模板 (Markdown 格式)

此 Markdown 文件可以直接通过 `rupost test <本文件名>` 运行。
RuPost 会自动解析并执行其中包含在 ` ```http ` 语法块中的 API 请求。

## 场景一：零门槛本地闭环测试 (Local Generic SSE Mock)

首先，在终端启动本地模拟服务：`rupost mock <本文件名> --port 8080`。
随后运行本文件，即可直接观察 SSE 解析与断言。

```http
# @sse
# @assert status == 200
# @assert stream.event == "build_start"
# @assert stream.llm.content contains "Compiling"
POST http://127.0.0.1:8080/build-stream
Content-Type: application/json

{
  "stream": true
}
```

## 场景二：流式响应实时增量保存到物理文件

通过 `@stream_to` 将接收到的推送日志实时追加或覆盖写入本地物理文件中。

```http
# @sse
# @stream_to ./target/build_log.txt overwrite
POST http://127.0.0.1:8080/build-stream
Content-Type: application/json

{
  "stream": true
}
```

---

## 场景三：本地通用 SSE 接口仿真 Mock 契约 (Mock Server Specification)

以下块不带任何断言，当在本地使用 `rupost mock <本文件名>` 启动 Mock 服务时，它会被自动识别并编译为 Mock 路由。配合 Mock 引擎的流式模拟输出，能够为接入通用 SSE 接口的客户端提供高度逼真的本地流式响应仿真。

```http
POST /build-stream
Content-Type: application/json

@mock-default
HTTP/1.1 200 OK
Content-Type: text/event-stream

event: build_start
data: {"status": "Compiling rupost v0.2.0..."}

event: build_progress
data: {"status": "Compiling crates/rupost-ai v0.2.0..."}

event: build_complete
data: {"status": "Finished build!"}
```
