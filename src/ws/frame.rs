use crate::variable::capture::VariableCapture;
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
    pub fn new(
        direction: FrameDirection,
        frame_type: WsFrameType,
        payload: Vec<u8>,
        timestamp_ns: u64,
    ) -> Self {
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
        /// 预编译好的 JSONPath segments
        segments: Option<Vec<String>>,
        /// 预编译出的期望值模板
        expected_value: Option<String>,
        /// 解析出的匹配运算符（如 "==", "!=", "contains"）
        operator: Option<String>,
        /// 匹配的超时时限
        timeout: Duration,
        /// 步骤级局部断言列表
        assertions: Vec<String>,
        /// 步骤级局部变量捕获列表
        captures: Vec<VariableCapture>,
    },
    /// 主动断开连接
    Close,
}

/// MessagePack 格式解码器（审查批一：唯一实现，trait 塌缩为固有方法；
/// 出现第二种解码器时再引入 trait）
pub struct MsgPackDecoder;

impl MsgPackDecoder {
    pub fn name(&self) -> &'static str {
        "messagepack"
    }

    pub fn decode(&self, payload: &[u8]) -> Result<serde_json::Value, String> {
        rmp_serde::from_slice(payload)
            .map_err(|e| format!("Failed to decode MessagePack payload: {}", e))
    }
}
