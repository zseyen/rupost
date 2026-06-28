# WebSocket 协议测试与调试 (WebSocket Testing & Debugging)

RuPost 原生支持纯 WebSocket 协议的长连接测试、订阅与会话级测试调试。它允许你在 `.http` 或 `.md` 中定义完整的 WebSocket 事件流剧本，并实现全自动断言。

---

## 1. 快速上手示例

在 `.http` 或 `.md` 代码块中，使用 `# @websocket` 指令声明这是一个长连接会话。通过 GET 方法发起握手：

```http
### 订阅 BTC 实时成交价
# @name WebSocket Basic Demo
# @websocket
# @assert body.price > 60000
# @capture btc_price from body.price
GET ws://localhost:8080/v1/market

# 1. 订阅动作发送 (默认发送 Outbound Text 帧)
SEND { "action": "subscribe", "topic": "ticker.btc" }

# 2. 阻塞式软匹配预期帧，支持 ==, !=, contains 运算符与变量引用
EXPECT $.event == "ticker"
@timeout = 3000

# 3. 混合匹配与包含运算符测试
EXPECT $.symbol contains "BT"
@timeout = 3000

# 4. 阻塞等待 500 毫秒后再进行下一步
WAIT 500

# 5. 主动优雅断开 WebSocket 连接
CLOSE
```

---

## 2. 剧本指令规范

* **`@websocket`**：用于元数据区，声明请求块为 WebSocket 剧本。请求方法必须为 `GET`，URL 协议必须为 `ws://` 或 `wss://`。
* **`SEND <payload>`**：向服务器发送数据帧（默认为 Text 帧）。Payload 能够跨多行书写，也支持 JSON 结构的隐式自动合并发送。支持使用双大括号进行变量插值（例如 `{{my_payload}}`）。
* **`EXPECT <condition>`**：阻塞式等待入站帧，直到满足条件或匹配超时。
  - **JSONPath 运算符匹配**：支持利用预编译 JSONPath 进行高级判定，包括：
    - `EXPECT $.event == "ticker"` (等于)
    - `EXPECT $.price != 1000` (不等于)
    - `EXPECT $.message contains "hello"` (包含子串)
  - **JSON 子集匹配**：自动解析为 JSON 结构，要求服务器返回的帧必须是期望 JSON 对象的超集（例如 `EXPECT {"status": "ok"}`）。
  - **模糊包含匹配**：如果 condition 不属于上述格式，退化为普通文本包含匹配（例如 `EXPECT pong`）。
  - **局部控制**：支持在此行下挂载局部指令（例如 `@timeout = 3000` 局部控制等待时间，以及针对本步骤的局部 `@assert` 与 `@capture` 变量提取）。
* **`WAIT <ms>`**：控制当前剧本执行线程休眠特定的毫秒数（如 `WAIT 1000`），避免高频请求压垮服务端。
* **`CLOSE`**：客户端发起优雅 Close 帧，通知服务器断开物理连接并停止会话。

---

## 3. 高级底座特性

### A. MessagePack 二进制转码器 (`@decoder`)
对于使用高效二进制传输（如金融行情广播）的 WebSocket 会话，可以在头部声明 `# @decoder messagepack`：
* 客户端会自动拦截入站二进制帧，并自适应调用 MessagePack 解码器将其还原为通用的 JSON 结构。
* 还原后的结构能完美透传给匹配引擎，使后续的 JSONPath 运算符匹配、步骤级 `@assert` 断言和 `@capture` 变量捕捉对二进制帧无感生效。

### B. 重连自愈与强时序 Flush 补发
物理长连接因断网等异常断开时，逻辑代理层 `WsSession` 会与后台守护 `SessionManager` 联动，执行以下自愈逻辑：
1. **指数退避自愈**：最大重试 5 次，退避时延为 2s, 4s, 8s, 16s, 32s (总计约 30 秒)。
2. **发送暂存缓冲**：重连挂起期间，外部发出的发帧请求将暂存于 `pending_send_queue`（容量上限 100 帧）。
3. **强时序 Flush (`push_front`)**：重连物理恢复的瞬间，立即对缓冲队列执行 `push_front` 补发，维持发送数据的绝对时序不颠倒、零丢失。

### C. 滑动历史环形缓冲区 (`BoundedFrameBuffer`)
会话物理收发的所有帧均会自动记录入历史缓冲区：
* 缓冲区设置了 1000 帧的滑动容量上限。
* 满额时采用 FIFO（先进先出）逻辑滑动顶替，从根本上杜绝了行情广播调试下由于大消息高频推送带来的 OOM 内存爆仓隐患，并能为后续的 TUI 提供平滑的历史回溯窗口。
