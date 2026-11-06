# 协议扩展体系——Mock、WebSocket 与 SSE 全貌 (Protocols & Mocking Engine)

本文档是 RuPost 项目关于多协议支持（HTTP/1.1、HTTP/2、WebSocket、SSE 流式长连接）以及本地轻量级动态 Mock 服务器的权威技术档案。整合自 Mock Server 设计规格、WebSocket 协议草案与流式断言实践，全面总结**业务需求、核心架构与算法演进、当前代码落地现状、后续演进路线**。

---

## 1. 业务需求与场景定义 (Why & What)

### 1.1 核心痛点与用户需求
现代 API 架构不仅包含传统的短连接请求/响应，还广泛集成了双向通信、事件推送与微服务依赖解耦场景：
1. **第三方服务依赖阻塞开发**：下游服务（如支付网关、短信接口）尚未联调或在 CI 环境中不可访问，开发者需要一个**“零额外配置、直接从现有测试用例自动生成”**的本地 Mock 服务器。
2. **动态多变体 Mock 诉求**：不仅需要返回固定 JSON，还需根据请求参数或 Header 分支匹配不同响应（如传入 `vip=true` 返回高级数据，传入空返回 400）。
3. **流式与实时协议断言**：AI 大模型时代普及的 SSE 流式打字机响应（Server-Sent Events）以及交易/即时通讯的 WebSocket 双向会话，传统工具无法在终端进行结构化断言和自动化回归。

### 1.2 语法契约与声明规范
- **语法 1：声明 Mock 变体与条件响应**：
  直接在现有 `.http` 文件中使用元数据扩展 Mock 行为：
  ```http
  ### 订单查询接口 (支持 Mock 变体)
  # @mock-path /api/orders/:id
  # @mock-variant vip {"query": {"vip": "true"}, "status": 200, "delay_ms": 50}
  # @mock-variant invalid {"query": {"id": "0"}, "status": 404}
  GET https://api.example.com/api/orders/1001
  ```
- **语法 2：启动独立 Mock 服务**：
  ```bash
  rupost mock ./tests/api.http --port 8080
  # 自动挂载 Trie 树路由，并在控制台实时输出带色彩的命中日志
  ```
- **语法 3：WebSocket 会话流断言**：
  ```http
  ### 订阅实时报价流
  WEBSOCKET wss://stream.example.com/quotes
  
  >>> {"action": "subscribe", "topic": "BTC_USDT"}
  <<< {"event": "subscribed", "status": "ok"}
  <<< {"price": "^[0-9]+(\\.[0-9]+)?$"}
  ```

---

## 2. 架构演进与核心算法设计 (Architecture & Evolution)

```mermaid
flowchart TD
    subgraph MockSystem ["1. 动态 Mock 架构 (src/mock/)"]
        HttpFiles[HTTP / MD 测试文件] --> MockCompiler[MockCompiler 路由编译器]
        MockCompiler --> TrieTree[Trie 路径参数树 (支持 :param 与 * 通配)]
        ClientReq[客户端请求到达] --> RouteMatch{Trie 树路径查找}
        RouteMatch -->|未命中| Err404[返回 404 Not Found]
        RouteMatch -->|命中路径| VariantMatcher[VariantMatcher 多分支条件评估]
        VariantMatcher -->|匹配成功| DelayInjector[延迟注入器 & 条件响应生成]
        DelayInjector --> AxumServer[Axum 高性能异步服务端]
    end

    subgraph RealtimeSystem ["2. 实时通信协议架构 (src/ws/ & src/http/)"]
        WsSession[WS 会话管理器] --> TungsteniteClient[tokio-tungstenite 异步通道]
        TungsteniteClient --> FrameParser[Action & Frame 解析器]
        FrameParser --> Matcher[流式断言匹配器]
        SseStream[SSE 长连接接收] --> EventChunk[Chunk 块解码与逐帧断言]
    end
```

### 2.1 架构设计与关键算法

#### 2.1.1 高性能 Trie 树模糊路由匹配算法 (`src/mock/trie.rs`)
为了在几纳秒内完成对复杂 RESTful 路径的解析与参数提取，Mock 引擎自研了前缀 Trie 树：
1. **节点分类**：分为静态节点（`orders`）、参数节点（`:id`）和通配符节点（`*path`）。
2. **贪婪回溯与参数抽取**：当请求 `/api/orders/2026/detail` 到达时，Trie 树自顶向下递归匹配，并在匹配参数节点时自动捕获键值映射 `id => "2026"` 注入给后续分支判断。

#### 2.1.2 变体匹配优先级模型 (`src/mock/matcher.rs`)
当同一个 URL 存在多个 Mock 分支时，引擎依据以下权重确定唯一命中响应：
$$\text{Weight} = 4 \times \text{BodyExactMatch} + 3 \times \text{HeaderMatch} + 2 \times \text{QueryParamMatch} + 1 \times \text{DefaultFallback}$$
确保最具体的测试分支优先命中，未定义条件则优雅回退至默认成功用例。

#### 2.1.3 全双工 WebSocket 会话与软匹配
针对 WebSocket 测试，实现了基于 `tokio-tungstenite` 的双工会话引擎：
- **操作符软匹配**：接收到的每一帧消息均通过通用断言引擎校验，支持正则匹配、字段包含与数值范围比对。
- **自愈重连机制**：在服务端发生短暂网络波动时，支持自动指数退避重连，确保长流程回归测试不被网络瞬断打断。

---

## 3. 目前已完成什么 (Status & Implementation)

| 核心组件 | 代码位置 | 职责说明 |
| :--- | :--- | :--- |
| **Mock 路径 Trie 树** | [`src/mock/trie.rs`](file:///Users/zsyzzx/project/rust/rupost/src/mock/trie.rs) | 实现动态路由挂载、参数（`:id`）提取与通配符支持 |
| **Mock 变体匹配器** | [`src/mock/matcher.rs`](file:///Users/zsyzzx/project/rust/rupost/src/mock/matcher.rs)<br>[`src/mock/variant.rs`](file:///Users/zsyzzx/project/rust/rupost/src/mock/variant.rs) | 根据 Query/Header/Body 条件精确分发多分支响应 |
| **Mock 编译器** | [`src/mock/compiler.rs`](file:///Users/zsyzzx/project/rust/rupost/src/mock/compiler.rs) | 从现有的 `.http` / `.md` AST 中编译出 Mock 服务端路由表 |
| **Axum 服务端适配器** | [`src/mock/server.rs`](file:///Users/zsyzzx/project/rust/rupost/src/mock/server.rs) | 提供高性能的异步 HTTP 服务端，记录请求日志与耗时 |
| **WebSocket 客户端** | [`src/ws/client.rs`](file:///Users/zsyzzx/project/rust/rupost/src/ws/client.rs)<br>[`src/ws/session.rs`](file:///Users/zsyzzx/project/rust/rupost/src/ws/session.rs) | 实现握手升级、全双工消息收发、帧超时控制与会话维护 |
| **CLI 命令行入口** | [`src/cli.rs`](file:///Users/zsyzzx/project/rust/rupost/src/cli.rs) | `rupost mock <FILE>` 命令支持 `--port`、`--watch` 热重载 |

### 验证与测试套件
- **单元测试**：`src/mock/trie.rs` 验证了各类边界路径的提取与冲突解决；`src/ws/action_parser.rs` 验证了会话剧本解析。
- **集成测试**：[`tests/mock_integration_test.rs`](file:///Users/zsyzzx/project/rust/rupost/tests/mock_integration_test.rs) 验证了 Mock 服务器启动、变体命中与延迟响应；[`tests/ws_integration_test.rs`](file:///Users/zsyzzx/project/rust/rupost/tests/ws_integration_test.rs) 验证了 WebSocket 完整链路交互。

---

## 4. 后续演进规划与待办 (Roadmap & Backlog)

1. **[P1] 双向流量录制生成 Mock 用例 (Record & Replay Mock)**：提供 `rupost record <URL>` 代理录制真实线上流量，自动转录生成具备多分支的 `.http` Mock 模板。
2. **[P2] OpenAPI 契约自动衍生 Mock**：解析 OpenAPI 3.0/3.1 规范中定义的 `examples` 和 `schema`，无需手写用例即可自动生成仿真 Mock 数据。
3. **[P2] SSE 流式断言语法糖深化**：为大语言模型输出的流式 Token 断言提供更友好的 `stream-assert` 校验操作符。
4. **[P3] gRPC 与 GraphQL 协议扩展调研**：根据生态演进，评估 Protobuf 编译与 gRPC 反射调用的接入方案。

---

## 5. 历史方案与设计文档追溯 (Historical Archive)

本档案完全吸收并承接了以下历史文档的核心内容与推演细节：
- `2026-06-06-mock-server-design.md`：Mock 服务器早期设计规格书。
- `2026-06-24-websocket-support-design.md`：WebSocket 协议调试与测试设计。
- [`mock_server.md`](../../guides/mock_server.md)：Mock 服务器使用指南。
- [`websocket_testing.md`](../../guides/websocket_testing.md)：WebSocket 调试实战手册。
- [`sse_testing.md`](../../guides/sse_testing.md)：Server-Sent Events 测试指南。
