# WebSocket 协议测试与调试 Markdown 文档示例

本示例展示了如何将 WebSocket 交互剧本直接写入 API 设计文档 Markdown 文件中。Rupost 能够自适应提取代码块并完成测试。

## 场景 1：基础 JSON 广播流订阅

下面的代码块是一个常规的 JSON WebSocket 剧本：

```http
### 订阅 BTC 实时成交价
# @name WebSocket MD JSON Demo
# @websocket
# @assert body.price > 60000
# @assert body.symbol == "BTC"
# @capture btc_price from body.price
GET ws://127.0.0.1:8080/v1/market

SEND { "action": "subscribe", "topic": "ticker.btc" }
EXPECT { "event": "ticker", "symbol": "BTC" }
@timeout = 3000
CLOSE
```

## 场景 2：MessagePack 二进制解码断言

对于使用 MessagePack 二进制传输的高频连接，可以使用 `@decoder messagepack` 进行自适应解码，解码后的内容将自动转化为 JSON 结构体：

```http
### 二进制 MsgPack 格式解码断言与捕获
# @name WebSocket MD MsgPack Demo
# @websocket
# @decoder messagepack
# @assert body.price == 3200
# @assert body.symbol == "ETH"
# @capture eth_price from body.price
GET ws://127.0.0.1:8080/v2/market_bin

SEND { "action": "subscribe_bin" }
EXPECT { "event": "ticker" }
@timeout = 3000
CLOSE
```
