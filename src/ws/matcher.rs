use crate::ws::frame::WsFrame;
use crate::utils::jsonpath::JsonPathResolver;
use serde_json::Value;

/// 解耦对接收帧匹配判定的接口
pub trait FrameMatcher {
    /// 判断给定的帧是否匹配该条件
    fn matches(&self, frame: &WsFrame, decoder: Option<&dyn crate::ws::PayloadDecoder>) -> bool;
}

/// JSONPath 匹配器，利用预编译的 segments 进行匹配
pub struct JsonPathMatcher {
    pub segments: Vec<String>,
    pub expected_value: Option<String>,
}

impl JsonPathMatcher {
    pub fn new(segments: Vec<String>, expected_value: Option<String>) -> Self {
        Self {
            segments,
            expected_value,
        }
    }
}

impl FrameMatcher for JsonPathMatcher {
    fn matches(&self, frame: &WsFrame, decoder: Option<&dyn crate::ws::PayloadDecoder>) -> bool {
        // 1. 如果有解码器，先尝试解码成 JSON Value
        let json_val = if let Some(dec) = decoder {
            match dec.decode(&frame.payload) {
                Ok(val) => val,
                Err(_) => return false,
            }
        } else {
            // 没有解码器，如果是 Text，直接转成 JSON Value。
            // 否则（是二进制但没配解码器），直接不匹配 JSONPath
            let txt = frame.payload_as_string();
            match serde_json::from_str::<Value>(&txt) {
                Ok(val) => val,
                Err(_) => return false,
            }
        };

        // 2. 利用预编译 segments 获取 JSON 节点值
        if let Some(target_val) = JsonPathResolver::resolve_with_segments(&json_val, &self.segments) {
            if let Some(ref expected) = self.expected_value {
                let target_str = match target_val {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    Value::Null => "null".to_string(),
                    _ => target_val.to_string(),
                };
                let clean_expected = expected.trim_matches('"').trim_matches('\'');
                let clean_target = target_str.trim_matches('"').trim_matches('\'');
                clean_target == clean_expected
            } else {
                // 如果没有指定期望值，只要 JSONPath 节点存在即算匹配成功（如 EXPECT $.event）
                true
            }
        } else {
            false
        }
    }
}

/// 默认的普通文本模糊子串匹配器，用于 Fallback
pub struct TextContainsMatcher {
    pub pattern: String,
}

impl FrameMatcher for TextContainsMatcher {
    fn matches(&self, frame: &WsFrame, _decoder: Option<&dyn crate::ws::PayloadDecoder>) -> bool {
        let txt = frame.payload_as_string();
        txt.contains(&self.pattern)
    }
}
