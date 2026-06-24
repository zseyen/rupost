---
base_path: /v1/chat/completions
---

# RuPost 大模型 (LLM) 测试与 Mock 快速起步模板

本文件提供可以直接复制的 LLM 测试和 Mock 服务模板，覆盖了流式匹配、多语言/复杂字符断言、流式内容本地落盘存储与变量捕获等常见场景。

---

## 📋 模板一：基础大模型流式连接测试 (Basic Connection)

最轻量的大模型 API 连接性流式测试模板：

```http
# @test
# @sse
# @assert status == 200
POST /
Content-Type: application/json
Authorization: Bearer {{env.API_KEY}}

{
  "model": "{{env.MODEL_NAME}}",
  "messages": [{"role": "user", "content": "Say hello."}],
  "stream": true
}
```

---

## 📋 模板二：复杂语言与格式健壮性测试 (Unicode & Formatting)

用于验证大模型在输出**中文、多字节特殊字符、表情符号 (Emoji)**时的正常流式传输与匹配断言。

```http
# @test
# @sse
# @assert status == 200
# @assert stream.llm.content contains "Rust"
# @assert stream.llm.content contains "语言"
# @assert stream.llm.content contains "😊"
POST /
Content-Type: application/json
Authorization: Bearer {{env.API_KEY}}

{
  "model": "{{env.MODEL_NAME}}",
  "messages": [
    {
      "role": "user",
      "content": "请用中文写一句话赞美 Rust 语言，必须包含“Rust”和“语言”这两个词，结尾加上 😊 符号。"
    }
  ],
  "stream": true
}
```

---

## 📋 模板三：流式响应本地落盘与下游传参 (Data Storage & Capture Relay)

本模板演示：
1. **数据存储**：利用 `# @stream_to` 将大模型的流式打字机内容**实时无回流写入/追加到本地物理文件**中。
2. **数据捕获与传递**：利用 `# @capture` 将流式文字提取为内存变量，在后续的业务请求中作为入参引用。

```http
# @test
# @sse
#   1. 将流式接收到的文本直接追加写入到指定的物理文件（自动创建目录）
# @stream_to ./target/llm_stream_output.txt overwrite
#
#   2. 流式结束后，将最终拼接的完整文本捕获为变量 `llm_reply`
# @capture llm_reply from stream.llm.content
POST /
Content-Type: application/json
Authorization: Bearer {{env.API_KEY}}

{
  "model": "{{env.MODEL_NAME}}",
  "messages": [{"role": "user", "content": "请为我写一份 100 字的 Rust 入门简介。"}],
  "stream": true
}

### 下游请求：将捕获到的 LLM 文本在业务接口中进行流转
# @test
POST https://httpbingo.org/post
Content-Type: application/json

{
  "doc_content": "{{llm_reply}}",
  "saved_by": "rupost-chain-tester"
}
```

> [!TIP]
> **📊 运行历史与数据展示说明**：
> 每次执行大模型流式测试，RuPost 都会自动在当前目录的 `.rupost/history/` 文件夹下生成一份 JSON 历史归档。
> 归档中包含每一个大模型 SSE 帧的传输耗时、原始 Event 报文以及最终合成的纯文本内容，帮助你可视化追溯大模型的输出轨迹。

---

## 📋 模板四：大模型流式接口本地仿真 Mock (LLM Mock Contract)

用于在没有网络或真实 Token 的情况下，在本地仿真出带有打字机效果的大模型 SSE 流式接口返回：

```http
POST /v1/chat/completions
Content-Type: application/json

@mock-default
HTTP/1.1 200 OK
Content-Type: text/event-stream
Cache-Control: no-cache
Connection: keep-alive

data: {"choices":[{"delta":{"content":"Rust"}}]}

data: {"choices":[{"delta":{"content":" 是一种"}}]}

data: {"choices":[{"delta":{"content":" 安全"}}]}

data: {"choices":[{"delta":{"content":"、高效的"}}]}

data: {"choices":[{"delta":{"content":" 系统级"}}]}

data: {"choices":[{"delta":{"content":" 编程语言。"}}]}

data: [DONE]
```

*   **启动 Mock 命令**：`rupost mock examples/llm_and_sse/llm_demo.md --port 8080`
*   **规则提示**：此语法块中没有 `# @test` 标记，所以在进行 `rupost test` 时它将被自动跳过 (skipped)，避免无网环境下报错。
