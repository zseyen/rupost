use serde::{Deserialize, Serialize};
use std::time::Duration;

/// WebSocket 帧的传输方向
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameDirection {
    /// 客户端发送给服务端
    Outbound,
    /// 服务端推送给客户端
    Inbound,
}

/// WebSocket 帧的基本类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WsFrameType {
    Text,
    Binary,
    Ping,
    Pong,
    Close,
}

/// 统一的 WebSocket 消息帧模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsFrame {
    /// 发生或接收的相对/绝对时间戳 (纳秒级)
    pub timestamp_ns: u64,
    /// 传输方向 (入站 / 出站)
    pub direction: FrameDirection,
    /// 帧类型 (Text / Binary / Ping / Pong / Close)
    pub frame_type: WsFrameType,
    /// 原始数据载荷
    pub payload: Vec<u8>,
}

impl WsFrame {
    /// 创建一个新的文本或二进制帧
    pub fn new(direction: FrameDirection, frame_type: WsFrameType, payload: Vec<u8>, timestamp_ns: u64) -> Self {
        Self {
            timestamp_ns,
            direction,
            frame_type,
            payload,
        }
    }

    /// 尝试将 payload 渲染为字符串
    pub fn payload_as_string(&self) -> String {
        String::from_utf8_lossy(&self.payload).into_owned()
    }
}

/// 剧本与回放中的 WebSocket 事件动作
#[derive(Debug, Clone)]
pub enum WsAction {
    /// 发起连接
    Connect {
        url: String,
        headers: Vec<(String, String)>,
    },
    /// 发送一帧
    Send(WsFrame),
    /// 挂起等待特定时间
    Wait(Duration),
    /// 阻塞等待并软匹配预期帧
    Expect {
        /// 匹配条件 (如 JSONPath 或普通文本包含)
        condition: String,
        /// 匹配的超时时限
        timeout: Duration,
    },
    /// 主动断开连接
    Close,
}

/// 载荷转码器接口，用于处理二进制与特定格式帧的转码断言
pub trait PayloadDecoder: Send + Sync {
    /// 转码器名称 (如 "messagepack")
    fn name(&self) -> &str;
    /// 将原始字节转码为通用的 JSON 结构以供断言和 TUI 渲染
    fn decode(&self, payload: &[u8]) -> Result<serde_json::Value, String>;
}

/// MessagePack 格式解码器实现
pub struct MsgPackDecoder;

impl PayloadDecoder for MsgPackDecoder {
    fn name(&self) -> &str {
        "messagepack"
    }

    fn decode(&self, payload: &[u8]) -> Result<serde_json::Value, String> {
        rmp_serde::from_slice(payload)
            .map_err(|e| format!("Failed to decode MessagePack payload: {}", e))
    }
}
