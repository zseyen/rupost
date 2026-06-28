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
    pub operator: Option<String>,
}

impl JsonPathMatcher {
    pub fn new(segments: Vec<String>, expected_value: Option<String>, operator: Option<String>) -> Self {
        Self {
            segments,
            expected_value,
            operator,
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
                
                let op = self.operator.as_deref().unwrap_or("==");
                match op {
                    "!=" => clean_target != clean_expected,
                    "contains" => clean_target.contains(clean_expected),
                    _ => clean_target == clean_expected,
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ws::frame::FrameDirection;
    use crate::ws::WsFrameType;
    use serde_json::json;

    #[test]
    fn test_jsonpath_matcher_operators() {
        let payload = json!({
            "event": "ticker",
            "price": 100,
            "message": "hello world"
        }).to_string().into_bytes();

        let frame = WsFrame::new(
            FrameDirection::Inbound,
            WsFrameType::Text,
            payload,
            0,
        );

        // 1. 测试 == 匹配
        let matcher_eq = JsonPathMatcher::new(
            vec!["event".to_string()],
            Some("\"ticker\"".to_string()),
            Some("==".to_string()),
        );
        assert!(matcher_eq.matches(&frame, None));

        // 2. 测试 != 匹配
        let matcher_ne = JsonPathMatcher::new(
            vec!["price".to_string()],
            Some("200".to_string()),
            Some("!=".to_string()),
        );
        assert!(matcher_ne.matches(&frame, None));

        let matcher_ne_fail = JsonPathMatcher::new(
            vec!["price".to_string()],
            Some("100".to_string()),
            Some("!=".to_string()),
        );
        assert!(!matcher_ne_fail.matches(&frame, None));

        // 3. 测试 contains 匹配
        let matcher_contains = JsonPathMatcher::new(
            vec!["message".to_string()],
            Some("world".to_string()),
            Some("contains".to_string()),
        );
        assert!(matcher_contains.matches(&frame, None));

        let matcher_contains_fail = JsonPathMatcher::new(
            vec!["message".to_string()],
            Some("java".to_string()),
            Some("contains".to_string()),
        );
        assert!(!matcher_contains_fail.matches(&frame, None));

        // 4. 测试无 expected_value 时节点存在即可
        let matcher_exist = JsonPathMatcher::new(
            vec!["event".to_string()],
            None,
            None,
        );
        assert!(matcher_exist.matches(&frame, None));

        let matcher_exist_fail = JsonPathMatcher::new(
            vec!["nonexistent".to_string()],
            None,
            None,
        );
        assert!(!matcher_exist_fail.matches(&frame, None));
    }
}
