---
base_path: /v1/chat/completions
---

# RuPost 大模型 (LLM) 测试与 Mock 快速起步模板

本文件提供可以直接复制的 LLM 测试和 Mock 服务模板。

---

## 📋 模板一：真实大模型接口流式测试 (LLM Test Template)

复制并修改以下代码块以快速建立你自己的真实大模型接口测试：

```http
# @test
# @sse
# @assert status == 200
# @assert stream.llm.content contains "Rust"
POST /
Content-Type: application/json
Authorization: Bearer {{env.API_KEY}}

{
  "model": "{{env.MODEL_NAME}}",
  "messages": [
    {
      "role": "user",
      "content": "请用一句话概括 Rust 语言的优势。"
    }
  ],
  "stream": true
}
```

*   **运行测试命令**：
    `rupost test examples/llm_and_sse/llm_demo.md`
*   **关键规则说明**：
    *   必须带有 `# @test` 标记，否则运行时该用例会被跳过 (skipped)。
    *   必须带有 `# @sse` 标记，指示 RuPost 启动 Server-Sent Events 流式解析。
    *   `stream.llm.content` 会自动提取大模型流式响应的 `choices[0].delta.content` 并拼接为完整文本，可以直接对其做 `contains` 校验。

---

## 📋 模板二：大模型流式接口本地仿真 Mock (LLM Mock Template)

复制以下代码块并修改返回的数据帧，可以在本地仿真出大模型接口的打字机流式响应：

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

*   **启动 Mock 服务命令**：
    `rupost mock examples/llm_and_sse/llm_demo.md --port 8080`
*   **关键规则说明**：
    *   **无 `# @test` 不执行**：此语法块中没有 `# @test` 声明，因此在执行 `rupost test` 时它将被跳过 (skipped)，保证测试的纯净性。
    *   **大模型流式返回格式**：上述 Response Body 中手写的 `data: {"choices":[{"delta":{"content":"..."}}]}` 即为大模型 SSE 流式接口返回的标准报文格式。
