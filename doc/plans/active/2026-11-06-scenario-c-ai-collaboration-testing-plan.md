# 场景 C（AI/SSE 流式打字机 + WebSocket 协同）测试规划与落地设计

> **状态**：Active  
> **创建日期**：2026-11-06  
> **关联分支/提交**：`.workspace/role-alignment-scenario`  
> **负责人**：架构师 (Architect) & Rust Async Implementation Expert  

---

## 1. 概述与业务画像

在现代 Web 应用与协同办公系统（如 AI 代码助手、实时协同画布、在线文档）中，系统往往由两种高阶网络协议共同支撑：
1. **AI/SSE 流式打字机 (Server-Sent Events)**：
   - 典型端点：`POST /api/v1/chat/completions`
   - 特性：客户端单向长连接接收、Chunk 分块推流（`text/event-stream`）、增量 Delta 文本合成、`[DONE]` 终止符、支持限流降级（429 Too Many Requests）。
2. **实时协同白板/画布 (WebSocket)**：
   - 典型端点：`GET /api/v1/collaboration/{channel}`
   - 特性：全双工、有状态连接、HTTP 101 Switching Protocols 升级、双向交互剧本（`>>>` 发送、`<<<` 匹配接收）、高频心跳自动过滤（Ping-Pong / Heartbeat）。

---

## 2. 现有代码调试模式支持度与差距分析

针对实际开发中的四大调试模式，结合当前代码库实际实现做出以下盘点与规划：

| 调试模式 | 协议场景 | 现状评估 | 对应代码与落地规划 |
| :--- | :--- | :---: | :--- |
| **模式一：CLI 深度诊断**<br>(`--debug` 逐帧解剖) | **SSE 流式** | **完全支持** | `src/runner/protocol/sse_runner.rs` 实现了帧编号、Payload 实时打印与 `@stream_to` 实时落盘。 |
| | **WebSocket** | **部分支持** | `src/runner/protocol/ws_runner.rs` 内部支持完整的动作时序与心跳软过滤，但 CLI 入口未格式化打印发送/接收时间戳。本方案将对其进行适度增强。 |
| **模式二：TUI 全交互会话**<br>(终端可视化面板) | **SSE 打字机** | **完全支持** | `src/tui/state/stream.rs` 中 `handle_stream_chunk` 实现了平滑折行与吸底滚动渲染。 |
| | **WebSocket** | **完全支持** | `src/tui/ws_interactive.rs` 实现了 `WsInteractiveSession` 常驻生命周期与双向帧路由。 |
| **模式三：双角色本地 Mock**<br>(零外部依赖自测) | **SSE 流式** | **完全支持** | `src/mock/server/axum_adapter.rs` 支持检测 `text/event-stream` 并通过 40ms 定时器逐行模拟打字机推流。 |
| | **WebSocket** | **暂未支持** | 目前 Mock 路由仅支持 HTTP/SSE。测试阶段采用 Test Double（Axum 0.8 本地内存/动态端口服务）隔离验证。 |
| **模式四：网络瀑布诊断**<br>(`rupost diagnose`) | **DNS/TCP/TLS/TTFB** | **完全支持** | `src/http/diagnose.rs` 支持全链路探测与 ASCII 彩色瀑布条，原生支持 `is_websocket` 握手诊断。 |

---

## 3. 契约规范与测试套件文件设计

遵循 Rupost 的规范，场景 C 的测试资产将落地在物理目录：
`examples/scenarios/scenario_c_ai_collaboration/`

### 3.1 `ai_spec.md`（单一真实源规范）
定义标准请求、Markdown 格式文档、双角色 Mock 变体（正常 SSE 流式推流与 429 降级）：
```markdown
# AI & Realtime Collaboration API Spec

## 1. AI Chat Completions (SSE)
```http
POST /api/v1/chat/completions
Content-Type: application/json
Accept: text/event-stream
@sse
@sse_timeout 5s
@stream_to test_scratch/scenario_c_chat.txt

{
  "model": "gpt-4o",
  "messages": [{"role": "user", "content": "Hello Rupost"}],
  "stream": true
}

@mock-when header.X-RateLimit-Test == 1
HTTP/1.1 429 Too Many Requests
Content-Type: application/json

{
  "error": { "code": "rate_limit_exceeded", "message": "Rate limit exceeded" }
}

@mock-default
HTTP/1.1 200 OK
Content-Type: text/event-stream

data: {"id":"1","choices":[{"delta":{"content":"Hello"}}]}

data: {"id":"2","choices":[{"delta":{"content":" from"}}]}

data: {"id":"3","choices":[{"delta":{"content":" Rupost!"}}]}

data: [DONE]
```

## 2. Realtime Canvas Collaboration (WebSocket)
```http
GET /api/v1/collaboration/canvas_101
@websocket
@timeout 5s
@decoder json

SEND {"action":"join","user":"designer_a"}
EXPECT {"event":"user_joined","user":"designer_a"}
SEND {"action":"draw","shape":"rectangle","x":100,"y":200}
EXPECT {"event":"shape_added","shape":"rectangle"}
CLOSE
```
```

### 3.2 `ai_test_suite.http`（自动化回归套件）
定义针对真实/Mock 服务的断言执行流：
```http
### 1. 验证 AI 流式打字机逐帧及增量合成
POST {{host}}/api/v1/chat/completions
Content-Type: application/json
Accept: text/event-stream
@sse
@sse_timeout 5s
@stream_to test_scratch/scenario_c_chat.txt

{
  "model": "gpt-4o",
  "messages": [{"role": "user", "content": "Hello Rupost"}],
  "stream": true
}

@assert status == 200
@assert headers.Content-Type contains "text/event-stream"
@assert stream.llm.content contains "Hello from Rupost!"

### 2. 验证 429 限流降级捕获
POST {{host}}/api/v1/chat/completions
Content-Type: application/json
X-RateLimit-Test: 1

{
  "model": "gpt-4o",
  "messages": [{"role": "user", "content": "Trigger rate limit"}]
}

@assert status == 429
@assert body.error.code == "rate_limit_exceeded"

### 3. 验证 WebSocket 全双工协同握手与动作断言
GET {{ws_host}}/api/v1/collaboration/canvas_101
@websocket
@timeout 5s
@decoder json

SEND {"action":"join","user":"designer_a"}
EXPECT {"event":"user_joined","user":"designer_a"}
SEND {"action":"draw","shape":"rectangle","x":100,"y":200}
EXPECT {"event":"shape_added","shape":"rectangle"}
CLOSE
```

---

## 4. 自动化集成测试设计（TDD 驱动）

在 `tests/scenarios_integration_test.rs` 中新增场景 C 专属用例：
1. **`test_scenario_c_spec_structure_and_mock_compilation`**：
   - 解析 `ai_spec.md`；
   - 验证 `@sse`、`@stream_to`、`@websocket` 等元数据与 WS actions 正确提取为 AST；
   - 验证编译器能将规范正确识别为包含两个 Mock 变体（200 与 429）的路由。
2. **`test_scenario_c_sse_dual_role_mock_and_execution`**：
   - 启动本地轻量 Axum 0.8 Mock 实例（支持延迟分片流与根据请求头降级）；
   - 使用 `TestExecutor` 执行 `ai_test_suite.http` 的 SSE 测试用例；
   - 验证 TTFB 延迟打点、逐帧捕获、`stream.llm.full` 文本拼接，并验证本地落盘文件 `test_scratch/scenario_c_chat.txt` 存在且内容正确。
3. **`test_scenario_c_ws_collaboration_e2e`**：
   - 启动包含心跳噪音广播（每 100ms 一次 ping）与协同业务响应的 Axum 0.8 / Tokio-Tungstenite WS 服务；
   - 执行 WS 剧本；
   - 验证连接完成 101 升级、上行 `join`/`draw` 成功派发、心跳被正确软过滤、`user_joined` 与 `shape_added` 成功匹配，连接安全关闭。

---

## 5. 验收标准（DoD）

- [ ] `doc/plans/active/2026-11-06-scenario-c-ai-collaboration-testing-plan.md` 规划文档已就绪。
- [ ] `examples/scenarios/scenario_c_ai_collaboration/` 包含规范的 `ai_spec.md` 与 `ai_test_suite.http`。
- [ ] 场景 C 的 AST、SSE 流式、429 降级及 WebSocket 双向协同测试在 `tests/scenarios_integration_test.rs` 中全部通过。
- [ ] 零外部网络、零外部模型依赖，单测与集成测试耗时在毫秒级。
- [ ] `cargo test` 100% 绿灯，`cargo clippy --all-targets --all-features` 零 Warning，`cargo fmt` 符合规范。
