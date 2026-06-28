# WebSocket 协议测试与调试实战指南

长连接与双向通信（WebSocket）在金融高频交易、币圈量化行情订阅、即时通信（IM）以及大模型 AI 流式推送等领域有着重度应用。然而，由于长连接具有**异步多线程推送、心跳频繁干扰、协议需协商升级、网络物理断线**等特性，使其在自动化测试中面临极高的调试与维护门槛。

本指南结合企业真实开发场景，为您提供从**连接鉴权约定、调试数据规约、现场数据复现到上线验收清单**的全链路标准化落地指导。

---

## 一、 开始阶段：连接与鉴权机制约定 (Connection & Auth Conventions)

在长连接项目立项或开发启动阶段，客户端与服务端必须对握手升级（Handshake）及安全鉴权机制达成强约定，并在测试剧本中予以映射。

### 1. Token 握手鉴权约定
约定鉴权信息携带于 HTTP 升级请求的头部，通常采用 `Authorization: Bearer <JWT>` 格式，或者为了防范反向代理对 Headers 的清洗，采用 Query 参数 `?token=<JWT>` 格式。

* **RuPost 对应剧本实现**：
  ```http
  ### 订阅安全行情通道
  # @websocket
  # @timeout = 3000
  GET wss://api.exchange.com/v1/stream?token={{my_jwt_token}}
  Authorization: Bearer {{my_jwt_token}}
  User-Agent: rupost/1.0.0
  ```

### 2. Cookie 会话继承约定
在 Web WebApp 中，WebSocket 必须继承用户此前在 HTTP 网页端登录后的 Session Cookie（通常为 `session_id`）。
* **自动化批处理传导约定**：
  通过 RuPost 的顺序批量执行模式，前置的 REST 登录脚本 `01_login.http` 成功执行后，会话 Cookie 自动留存入 `CookieStore`。第二步执行的 `02_websocket.http` 无需手动写 Cookie 头部，RuPost 的中间件管道会在 Upgrade 物理建连前自动将 Cookie 注入 WebSocket 握手请求包中，达成无感状态流转。
  ```http
  # 02_websocket.http
  # @websocket
  GET wss://api.exchange.com/v1/user/portfolio
  # 握手时自动注入登录成功的 Cookie: session_id=abc123xyz
  ```

### 3. 101 Upgrade 协商超时规约
为了防范由于网络吊死或防火墙（GFW）静默丢包导致的测试线程无限卡死，约定长连接握手阶段的最长物理等待为 `3000ms`（通过 `@timeout` 局部控制或 metadata 全局配置限制）。

---

## 二、 调试阶段：数据结构与 Payload 转码约定 (Data Structure Conventions)

长连接在联调和调试阶段，收发的数据结构必须符合强类型的消息协议规约（Framing Protocol）。

### 1. 消息外壳格式约定 (Envelope Framing)
为了能利用统一的匹配引擎进行帧分发，约定所有出入站的 JSON 消息必须包含 `event`（事件类型）或 `msg_type` 字段作为路由判定依据。
* 消息外壳定义示例：
  ```json
  {
    "event": "ticker",
    "timestamp": 1719590400000,
    "data": {
      "symbol": "BTC_USDT",
      "price": 62500.00
    }
  }
  ```

### 2. 二进制数据类型序列化对齐约定 (MsgPack / Protobuf)
在使用 MessagePack 或 Protobuf 传输二进制高频数据时，必须提供特定的编解码规范：
* 二进制解码后的 JSON 数据，其键名和数值类型（如浮点数精度、空值表达形式）需与 REST API 保持完全一致。
* **RuPost 对应转码断言实现**：
  在剧本头部通过 `# @decoder messagepack` 声明。当 Binary 字节包入站时，底层的 `MsgPackDecoder` 会自适应将其转译为上述 JSON 外壳结构，从而使 `EXPECT $.data.symbol == "BTC_USDT"` 和 `@assert body.data.price > 60000` 能够完全无感地工作。

---

## 三、 测试与联调阶段：5 大核心实战场景与调试示例

### 场景一：建立连接与 Nginx 101 反代故障排障诊断
在部署长连接服务时，最常见的报错是客户端返回 `Connection failed`，而 Nginx 返回 `400 Bad Request` 或 `502 Bad Gateway`。这是因为 Nginx 默认不会转发 Upgrade 头部，导致物理协议协商失败。

* **使用诊断工具定位**：
  在终端中运行 `diagnose` 子命令：
  ```bash
  rupost diagnose ws://api.exchange.com/v1/stream
  ```
  **实战诊断控制台输出示例**：
  ```
  [PROBING] Physical Connection... OK
  [PROBING] 101 Upgrade Handshake Negotiation... FAILED (HTTP Status: 400 Bad Request)
  [DIAGNOSIS] Upgrade header is missing in remote response!
  [HINT] Please ensure Nginx server block contains:
         proxy_set_header Upgrade $http_upgrade;
         proxy_set_header Connection "upgrade";
  ```
  通过这种细粒度诊断，你可以秒级定位是物理网路阻断，还是 Nginx 反代配置漏配了协议升级头。

### 场景二：常规 JSON 格式与多行排版定义
常规的文本格式帧交互，支持 JSON 缩进及多行 SEND 定义：
```http
# @websocket
GET ws://127.0.0.1:8080/v1/market

# 显式发送 JSON
SEND {
  "action": "subscribe",
  "topic": "ticker",
  "options": {
    "depth": 10
  }
}

EXPECT { "event": "ticker", "symbol": "BTC" }
CLOSE
```

### 场景三：步骤级变量级联流动与自动化下单测试 (Variable Cascade)
用例需求：订阅 BTC_USDT 实时行情，捕获最新成交价，并在最新价的基础上减去 $10 作为买单限价，向交易网关发起限价委托，最后验证委托是否成功。

```http
### 委托下单级联状态机测试
# @websocket
GET wss://api.exchange.com/v1/trade_stream

# 1. 发帧订阅 BTC_USDT 行情
SEND { "action": "subscribe", "symbol": "BTC_USDT" }

# 2. 等待返回 ticker 行情帧，从 body 中捕获当前的 price 并存入变量 btc_price
EXPECT $.event == "ticker"
@timeout = 3000
@capture btc_price from body.price

# 3. 发帧执行委托买入指令。RuPost 会在发帧的瞬间，自动将 {{btc_price}} 替换为第二步捕获的值！
SEND { "action": "place_order", "symbol": "BTC_USDT", "side": "buy", "price": "{{btc_price}}", "qty": 0.05 }

# 4. 阻塞匹配订单状态确认帧
EXPECT $.event == "order_status"
@timeout = 4000
@assert body.status == "new"
@assert body.executed_qty == 0

# 5. 主动断开连接
CLOSE
```

### 场景四：高频心跳广播杂音的隔离与软匹配过滤 (Heartbeat Bypassing)
在行情或 IM 系统中，服务端为了维持 TCP 活性，通常每 100ms ~ 1000ms 就会向客户端广播一条心跳心跳帧，例如：`{"type": "heartbeat", "timestamp": 1719590400}` 或 `ping`。

如果普通的测试工具遇到这些广播，会发生断言误伤（因为预期的是交易成交包，接到的确是高频心跳包导致断言不符崩溃）。

* **规避心跳杂音实战剧本**：
  ```http
  ### 心跳杂音过滤演示
  # @websocket
  GET ws://localhost:8080/v1/chat
  
  # 假设此时服务器一直在高频推送 {"type":"heartbeat"}
  
  SEND { "action": "send_msg", "text": "hello" }
  
  # 1. 软匹配隔离：此 EXPECT 只有当收到包含 event 键且值为 message_ack 的帧时才会被唤醒！
  # 2. 期间到达的所有高频心跳帧，都会被匹配引擎静默吞吞并丢弃，不会干扰测试进度。
  EXPECT $.event == "message_ack"
  @timeout = 3000
  # 3. 局部断言安全边界：此断言只在本 EXPECT 被唤醒命中时才会执行，绝对不会被高频心跳误伤！
  @assert body.success == true
  
  CLOSE
  ```
  **原理解析**：
  RuPost 的匹配引擎会在收到每一物理帧时进行 `FrameMatcher::matches` 条件检测。如果检测失败，该帧仅被归档存入滑动历史缓冲区，而不会唤醒当前的 `EXPECT` 锁。局部断言 `@assert` 和 `@capture` 挂载在相应的 `EXPECT` 指令下，因此它们拥有独立的运行上下文，仅当对应步骤被正确命中时才触发，完美抵御了高频行情心跳雪崩的误伤。

### 场景五：网络抖动重连自愈与强时序 Flush 补发调试
物理长连接因断网等异常断开时，逻辑代理层 `WsSession` 会与后台守护 `SessionManager` 自动激活混沌自愈：
1. **指数退避重连**：最大重试 5 次（约 30 秒），退避时延为 2s, 4s, 8s, 16s, 32s。
2. **发送暂存缓冲**：重连挂起期间，外部发出的发帧请求将暂存于 `pending_send_queue`（限制 100 帧容量，防 OOM）。
3. **强时序 Flush (`push_front`)**：重连物理恢复的瞬间，立即对缓冲队列执行 `push_front` 补发，维持发送数据的绝对时序不颠倒、零丢失。

---

## 四、 测试阶段：现场流量记录、排障与数据复现 (Reproducibility)

在分布式系统和长连接测试中，最难以定位的是“偶发性”及“难以复现”的协议错误。RuPost 通过会话记录器保障了高水准的现场复现能力。

### 1. 滑动内存限制历史帧环形缓冲区 (`BoundedFrameBuffer`)
无论网络是否抖动，收发的所有帧均会自动记录入逻辑 `history_buffer`（容量 1000 帧），满额时自动执行 FIFO 滑动顶替。这既防止了高频行情调试时的内存泄露，又提供了全量会话帧的可回溯历史。

### 2. 现场流量导出与本地 100% 数据复现流程
当自动化测试在 CI/CD 中因未知协议包报错时，可以通过日志或快照命令将这 1000 帧历史流导出为 `ws_snapshot.json`：
```json
[
  { "timestamp_ns": 1000200, "direction": "Outbound", "frame_type": "Text", "payload": "{\"action\":\"subscribe\"}" },
  { "timestamp_ns": 1000450, "direction": "Inbound", "frame_type": "Text", "payload": "{\"type\":\"heartbeat\"}" },
  { "timestamp_ns": 1000990, "direction": "Inbound", "frame_type": "Text", "payload": "{\"event\":\"ticker\",\"price\":62500}" }
]
```
* **本地重放复现**：
  在本地调试时，只需将该快照配置进本地 `rupost mock` 服务，Mock 引擎会自动按照其中的时间偏移和数据载荷还原出当时 CI 现场一模一样的推送流量。开发人员可在本地连接此 Mock 桩，以完全一致的状态重放剧本，实现 100% 故障复现。

---

## 五、 现场验收测试清单 (On-site Acceptance Checklist)

在 WebSocket 长连接应用进行现场部署、升级或上线前，必须执行以下标准化验收测试清单，并跑通对应剧本：

| 验收项目 | 校验方法与期望结果 | 状态 | 对应测试工具/剧本 |
| :--- | :--- | :--- | :--- |
| **Nginx Upgrade 协商** | Nginx 反代配置能正确进行协议升级，`rupost diagnose` 时 101 Handshake 响应通过且包含正确的 Upgrade/Connection 头部。 | [ ] | `rupost d ws://...` |
| **过期/非法鉴权防御** | 使用过期 JWT 或被篡改的 JWT 握手，服务端必须返回 `401 Unauthorized` 拒绝升级，客户端剧本能捕获报错。 | [ ] | 编写带失效 Token 的握手脚本 |
| **高频心跳防雪崩** | 服务端以 100ms 频率推送心跳帧，客户端持续发消息时，脚本的断言和变量捕获不应因心跳包产生任何干扰或中断。 | [ ] | 带有 `@assert` 的高频 EXPECT 剧本 |
| **物理断线自愈补发** | 在脚本运行中强行关闭服务端 5 秒后重启，客户端应自动指数退避建连成功；重连期间发送的指令必须以 `push_front` 时序无损补发。 | [ ] | 模拟混沌断网测试 |
| **滑动环形历史区** | 常规行情下持续运行 15 分钟以上，客户端进程物理内存占用保持稳定，且通过 `history` 查询的历史帧数量严格限制在 1000 帧以内。 | [ ] | 压力连结测试 |
