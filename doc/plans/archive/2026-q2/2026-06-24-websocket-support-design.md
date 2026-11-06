# Rupost WebSocket 调试与测试设计规格书

本设计文档确立了 Rupost 项目中 WebSocket 调试与测试模块的功能定义、技术实现方案与架构规范，旨在将 “文档即测试，测试即文档” 的核心愿景延伸至长连接与双向通信领域。

---

## 一、 业务场景与用户故事 (User Stories)

### 1. 角色画像与核心场景
- **后端/接口开发工程师（小张）**：在联调配置下发与状态监控服务时，需快速连接，下发预设 JSON payload 并实时过滤/暂停滚动查看数据，以验证接口契约。
- **测试/质量保证工程师（小李）**：在编写集成测试脚本时，需支持鉴权级联，能够在 CI/CD 中进行自动流式断言；在遇到偶发时序 Bug 时，能录制现场流量（快照）并在测试环境按时序重放以重现故障。
- **前端/客户端开发工程师（小王）**：需要消费推送服务，在后端接口未就绪或脱机网络下，能利用 Rupost 的 Server Mock 模式回放历史快照来调试前端 UI。

### 2. 实际用例规范

#### 用例一：前端活文档学习与即时联调用例
在 Markdown (`doc/api/chat_room.md`) 中，既作为可读文档呈现，又支持一键交互式调试：

```markdown
# 实时聊天室 WebSocket 接口文档

## 1. 连接聊天室
```http
### 连接聊天室
@websocket
@name = JoinChatRoom
GET ws://{{base_url}}/chat?token={{auth_token}}
```

## 2. 加入房间与订阅消息
```http
### 加入聊天房间并订阅消息
@websocket
@name = SubscribeRoomMessages
GET ws://{{base_url}}/chat?token={{auth_token}}

# 发送加入房间帧
SEND {
  "action": "join",
  "room_id": "room_101"
}

# 软匹配：忽略心跳包，只等待房主欢迎消息
EXPECT { "event": "user_joined", "room_id": "room_101" }
@timeout = 3000
@assert message.body.user.role == "owner"

# 持续监控接收：捕获聊天消息 Payload 供后续分析
EXPECT { "event": "new_message" }
@capture latest_chat_msg = message.body
```
```

#### 用例二：二进制 / MsgPack 帧的解码调试用例 (`iot_sensor.http`)
```http
### IoT 传感器高频二进制数据订阅
@websocket
@name = IoTSensorDebug
@decoder = messagepack
GET ws://127.0.0.1:8080/devices/sensors

SEND {
  "device_id": "temp_sensor_01",
  "command": "start_stream"
}

EXPECT { "device_id": "temp_sensor_01" }
@timeout = 2000
@assert message.body.temperature > 0.0
@assert message.body.status == "normal"
```

#### 用例三：现场回放与单步交互式调试
通过 CLI 对录制的 `crash_log.jsonl` 快照按原有时延差，并参数化注入新 token 后交互式发送：
```bash
rupost ws replay crash_log.jsonl --role client --var auth_token=dev_token_123 --interactive
```

---

## 二、 核心架构设计 (Clean Architecture)

```
src/
└── protocol/
    └── ws/
        ├── frame.rs       # [实体层] 定义统一的 WsFrame, WsAction 核心模型与解码器
        ├── client.rs      # [网关层] 封装无状态的 WsClient (基于 tokio-tungstenite)
        ├── session.rs     # [领域层] 维护当前会话的帧缓存与快照序列化导出
        ├── engine.rs      # [用例层] 统一的 WsActionStream 驱动与断言匹配器
        └── ui/
            └── ws_console.rs # [表现层] 交互式调试大纲导航面板与过滤时间线
```

### 1. 统一的数据结构 (`src/protocol/ws/frame.rs`)
```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WsFrame {
    pub timestamp_ns: u64,
    pub direction: FrameDirection, // Inbound (接收) / Outbound (发送)
    pub frame_type: WsFrameType,   // Text / Binary / Ping / Pong / Close
    pub payload: Vec<u8>,          // 二进制或文本的载荷字节
}

#[derive(Debug, Clone)]
pub enum WsAction {
    Connect { url: String, headers: Vec<(String, String)> },
    Send(WsFrame),
    Wait(std::time::Duration),
    Expect { condition: String, timeout: std::time::Duration },
    Close,
}
```

### 2. 双层接收架构 (Dual-Layer Worker)
解决 `EXPECT` 阻塞断言等待时心跳中断的问题：
1. **后台 Worker**：底层 `WsTunnel` 在建连后派生一个后台 Tokio 协程，持续从 Socket 读取帧。该协程自动响应 Ping/Pong 和业务定义心跳 JSON，并把帧写入广播通道。同时向内存 Session 无损记录历史。
2. **广播通道**：使用 `tokio::sync::broadcast` 进行消息派发。
3. **用例匹配器 (`ExpectMatcher`)**：主执行引擎（WsEngine）从通道订阅数据，在 `@timeout` 时间内进行过滤，不匹配条件的消息直接忽略，保证正常心跳响应。

### 3. 解码转码层 (`PayloadDecoder`)
- 设计 `PayloadDecoder` 插件化接口。对于声明了 `@decoder = messagepack` 等属性的块，在 Binary 帧到达时自动调用 `rmp-serde` 反序列化为 `serde_json::Value`，供 TUI 渲染和表达式断言引擎使用。
- **降级容错 (Fallback)**：转码失败时，不抛出中断，而是将 payload 降级转换为 Hex 编码文本呈现，并支持使用 `message.raw_bytes` 对原始字节段进行下标匹配（如 `message.raw_bytes[0] == 0x82`）。

### 4. 故障时序录制与回放引擎
- **录制**：静默收集会话帧并按 JSON Lines (`.jsonl`) 逐行序列化至磁盘。
- **回放模式**：
  - **Client 模式（故障复现）**：计算快照帧间的时间差 $\Delta t$，挂起发送。支持利用 Rupost 模板引擎对文本 payload 中的占位符进行参数化变量覆盖。
  - **Server 模式（脱机 Mock）**：通过 `tokio::net::TcpListener` 监听端口，将快照中入站的接收帧依次发送给连接的本地客户端。

### 5. 调试专属能力 (TUI / CLI 增强)
- **TUI Markdown 大纲模式**：在 TUI 模式下加载 Markdown 文档时，左侧渲染出大纲导航栏，方便前端双击直接建连和调试不同的主题订阅。
- **TUI 草稿区与 Live 编辑**：提供 Draft 编辑缓冲，支持使用快捷键联动外部终端编辑器（如 `vim`）修改长 payload。
- **异常链路模拟**：支持向后台 Worker 注入掉线、阻断心跳响应状态，以便测试重连及异常包恢复。

---

## 三、 第三方技术栈选型

- **`tokio-tungstenite`**：异步长连接与帧传输。
- **`ratatui`**：终端 TUI 框架与场景大纲布局。
- **`rmp-serde`**：MessagePack 反序列化为 Serde 结构。
- **`jsonpath-rust`**：JSONPath 实时过滤。
- **`serde_json`**：快照 JSONL 序列化与美化渲染。
