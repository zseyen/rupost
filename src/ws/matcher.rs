use crate::utils::jsonpath::JsonPathResolver;
use crate::ws::frame::WsFrame;
use serde_json::Value;
use crate::assertion::{AssertTarget, AssertError, AssertValue, AssertExpr, CompareOp, ValuePath, evaluate_assertion, json_value_to_assert_value, parse_assert_value};

pub struct WsFrameTarget<'a> {
    pub frame: &'a WsFrame,
    pub decoder: Option<&'a dyn crate::ws::PayloadDecoder>,
}

impl<'a> AssertTarget for WsFrameTarget<'a> {
    fn extract_status(&self) -> Result<f64, AssertError> {
        Err(AssertError::PathNotFound("status not applicable for WebSocket frames".to_string()))
    }

    fn extract_header(&self, name: &str) -> Result<String, AssertError> {
        if name.eq_ignore_ascii_case("type") || name.eq_ignore_ascii_case("frame_type") {
            Ok(format!("{:?}", self.frame.frame_type))
        } else if name.eq_ignore_ascii_case("direction") {
            Ok(format!("{:?}", self.frame.direction))
        } else {
            Err(AssertError::PathNotFound(format!("Header '{}' not applicable for WebSocket frames", name)))
        }
    }

    fn extract_body(&self, segments: &[String]) -> Result<AssertValue, AssertError> {
        let json_val = if let Some(dec) = self.decoder {
            dec.decode(&self.frame.payload)
                .map_err(|e| AssertError::ExtractionError(format!("Failed to decode payload: {}", e)))?
        } else {
            let txt = self.frame.payload_as_string();
            serde_json::from_str(&txt).map_err(|e| AssertError::ExtractionError(format!("Failed to parse JSON: {}", e)))?
        };

        let target_val = JsonPathResolver::resolve_with_segments(&json_val, segments)
            .ok_or_else(|| AssertError::PathNotFound(format!("Path 'body.{}' not found", segments.join("."))))?;
        
        json_value_to_assert_value(&target_val)
    }

    fn extract_response_time(&self) -> Result<f64, AssertError> {
        Err(AssertError::PathNotFound("response.time not applicable for WebSocket frames".to_string()))
    }

    fn extract_stream_event(&self) -> Result<String, AssertError> {
        Err(AssertError::PathNotFound("stream.event not applicable for WebSocket frames".to_string()))
    }

    fn extract_stream_id(&self) -> Result<String, AssertError> {
        Err(AssertError::PathNotFound("stream.id not applicable for WebSocket frames".to_string()))
    }

    fn extract_stream_body(&self, _segments: &[String]) -> Result<AssertValue, AssertError> {
        Err(AssertError::PathNotFound("stream.body not applicable for WebSocket frames".to_string()))
    }

    fn extract_stream_llm_content(&self) -> Result<String, AssertError> {
        Err(AssertError::PathNotFound("stream.llm.content not applicable for WebSocket frames".to_string()))
    }

    fn extract_timing_dns(&self) -> Result<f64, AssertError> {
        Err(AssertError::PathNotFound("timing.dns not applicable for WebSocket frames".to_string()))
    }

    fn extract_timing_tcp(&self) -> Result<f64, AssertError> {
        Err(AssertError::PathNotFound("timing.tcp not applicable for WebSocket frames".to_string()))
    }

    fn extract_timing_tls(&self) -> Result<f64, AssertError> {
        Err(AssertError::PathNotFound("timing.tls not applicable for WebSocket frames".to_string()))
    }

    fn extract_timing_ttfb(&self) -> Result<f64, AssertError> {
        Err(AssertError::PathNotFound("timing.ttfb not applicable for WebSocket frames".to_string()))
    }

    fn extract_cert_days(&self) -> Result<f64, AssertError> {
        Err(AssertError::PathNotFound("cert.days not applicable for WebSocket frames".to_string()))
    }

    fn extract_cert_issuer(&self) -> Result<String, AssertError> {
        Err(AssertError::PathNotFound("cert.issuer not applicable for WebSocket frames".to_string()))
    }

    fn extract_cert_subject(&self) -> Result<String, AssertError> {
        Err(AssertError::PathNotFound("cert.subject not applicable for WebSocket frames".to_string()))
    }
}

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
    pub fn new(
        segments: Vec<String>,
        expected_value: Option<String>,
        operator: Option<String>,
    ) -> Self {
        Self {
            segments,
            expected_value,
            operator,
        }
    }
}

impl FrameMatcher for JsonPathMatcher {
    fn matches(&self, frame: &WsFrame, decoder: Option<&dyn crate::ws::PayloadDecoder>) -> bool {
        let expr = if let Some(ref expected) = self.expected_value {
            let op_str = self.operator.as_deref().unwrap_or("==");
            let op = CompareOp::parse(op_str).unwrap_or(CompareOp::Equal);
            let right = parse_assert_value(expected)
                .unwrap_or_else(|_| AssertValue::String(expected.clone()));
            AssertExpr::Compare {
                left: ValuePath::Body(self.segments.clone()),
                op,
                right,
            }
        } else {
            AssertExpr::Exists {
                path: ValuePath::Body(self.segments.clone()),
            }
        };

        let target = WsFrameTarget { frame, decoder };
        let result = evaluate_assertion(&expr, &target);
        result.passed
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

/// 综合条件匹配器，负责分发 JSONPath 匹配、JSON 子集匹配和纯文本包含匹配
pub struct WsConditionMatcher {
    condition: String,
    condition_json: Option<Value>,
    jsonpath_matcher: Option<JsonPathMatcher>,
}

impl WsConditionMatcher {
    pub fn compile(
        condition: &str,
        segments: &Option<Vec<String>>,
        expected_value: &Option<String>,
        operator: &Option<String>,
    ) -> Self {
        let condition = condition.trim().to_string();
        let condition_json = serde_json::from_str::<Value>(&condition).ok();
        let jsonpath_matcher = segments.as_ref().map(|segs| {
            JsonPathMatcher::new(segs.clone(), expected_value.clone(), operator.clone())
        });
        Self {
            condition,
            condition_json,
            jsonpath_matcher,
        }
    }
}

impl FrameMatcher for WsConditionMatcher {
    fn matches(&self, frame: &WsFrame, decoder: Option<&dyn crate::ws::PayloadDecoder>) -> bool {
        if self.condition.is_empty() {
            return true;
        }

        // 1. 如果有预编译的 jsonpath_matcher，直接匹配
        if let Some(ref matcher) = self.jsonpath_matcher {
            return matcher.matches(frame, decoder);
        }

        // 2. 如果 condition_json 是一个合法的 JSON，且接收帧也是合法 JSON，尝试执行 JSON 子集匹配
        if let Some(ref cond_val) = self.condition_json {
            let msg_str = if let Some(dec) = decoder {
                match dec.decode(&frame.payload) {
                    Ok(val) => match val {
                        Value::String(ref s) => s.clone(),
                        _ => val.to_string(),
                    },
                    Err(_) => frame.payload_as_string(),
                }
            } else {
                frame.payload_as_string()
            };

            if let Ok(msg_val) = serde_json::from_str::<Value>(&msg_str) {
                return match_json_subset(cond_val, &msg_val);
            }
        }

        // 3. 否则，降级使用子字符串包含匹配
        let txt = frame.payload_as_string();
        txt.contains(&self.condition)
    }
}

/// 检查 pattern 是否为 target 的子集
fn match_json_subset(pattern: &Value, target: &Value) -> bool {
    match (pattern, target) {
        (Value::Object(pat_obj), Value::Object(tgt_obj)) => {
            for (k, v) in pat_obj {
                match tgt_obj.get(k) {
                    Some(tgt_v) => {
                        if !match_json_subset(v, tgt_v) {
                            return false;
                        }
                    }
                    None => return false,
                }
            }
            true
        }
        (Value::Array(pat_arr), Value::Array(tgt_arr)) => {
            if pat_arr.is_empty() {
                return true;
            }
            pat_arr
                .iter()
                .all(|p| tgt_arr.iter().any(|t| match_json_subset(p, t)))
        }
        (p, t) => p == t,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ws::WsFrameType;
    use crate::ws::frame::FrameDirection;
    use serde_json::json;

    #[test]
    fn test_jsonpath_matcher_operators() {
        let payload = json!({
            "event": "ticker",
            "price": 100,
            "message": "hello world"
        })
        .to_string()
        .into_bytes();

        let frame = WsFrame::new(FrameDirection::Inbound, WsFrameType::Text, payload, 0);

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
        let matcher_exist = JsonPathMatcher::new(vec!["event".to_string()], None, None);
        assert!(matcher_exist.matches(&frame, None));

        let matcher_exist_fail = JsonPathMatcher::new(vec!["nonexistent".to_string()], None, None);
        assert!(!matcher_exist_fail.matches(&frame, None));
    }

    #[test]
    fn test_ws_condition_matcher() {
        let payload = json!({
            "event": "ticker",
            "symbol": "BTC",
            "price": 60000
        })
        .to_string()
        .into_bytes();

        let frame = WsFrame::new(FrameDirection::Inbound, WsFrameType::Text, payload, 0);

        // 1. 测试 JSONPath 编译与匹配
        let matcher = WsConditionMatcher::compile(
            "$.symbol == BTC",
            &Some(vec!["symbol".to_string()]),
            &Some("BTC".to_string()),
            &Some("==".to_string()),
        );
        assert!(matcher.matches(&frame, None));

        // 2. 测试 JSON 子集匹配
        let matcher =
            WsConditionMatcher::compile(r#"{"event":"ticker","price":60000}"#, &None, &None, &None);
        assert!(matcher.matches(&frame, None));

        // 测试不匹配的 JSON 子集
        let matcher =
            WsConditionMatcher::compile(r#"{"event":"ticker","price":50000}"#, &None, &None, &None);
        assert!(!matcher.matches(&frame, None));

        // 3. 测试纯文本包含匹配
        let matcher = WsConditionMatcher::compile("BTC", &None, &None, &None);
        assert!(matcher.matches(&frame, None));

        let matcher = WsConditionMatcher::compile("ETH", &None, &None, &None);
        assert!(!matcher.matches(&frame, None));

        // 4. 空条件匹配
        let matcher = WsConditionMatcher::compile("", &None, &None, &None);
        assert!(matcher.matches(&frame, None));
    }
}
