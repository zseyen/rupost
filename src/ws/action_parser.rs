use crate::ws::frame::{FrameDirection, WsAction, WsFrame, WsFrameType};
use std::time::Duration;

pub struct WsActionParser;

impl WsActionParser {
    /// 将剧本 body 文本流解析为结构化的 WsAction 队列
    pub fn parse_body(body_str: &str) -> Result<Vec<WsAction>, String> {
        let mut actions = Vec::new();
        let mut lines = body_str.lines().peekable();

        while let Some(line) = lines.next() {
            let trimmed = line.trim();
            // 跳过空行和注释行
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
                continue;
            }

            if let Some(after_send) = trimmed.strip_prefix("SEND") {
                // 1. 发送帧动作
                let mut payload_str = after_send.trim().to_string();
                if payload_str.is_empty() {
                    // 多行读取，直到遇到下一个核心指令
                    let mut accumulated = String::new();
                    while let Some(next_line) = lines.peek() {
                        let next_trimmed = next_line.trim();
                        if next_trimmed.starts_with("SEND")
                            || next_trimmed.starts_with("EXPECT")
                            || next_trimmed.starts_with("WAIT")
                            || next_trimmed.starts_with("CLOSE")
                        {
                            break;
                        }
                        accumulated.push_str(lines.next().unwrap());
                        accumulated.push('\n');
                    }
                    payload_str = accumulated.trim().to_string();
                }

                let frame = WsFrame::new(
                    FrameDirection::Outbound,
                    WsFrameType::Text,
                    payload_str.into_bytes(),
                    0,
                );
                actions.push(WsAction::Send(frame));
            } else if let Some(after_expect) = trimmed.strip_prefix("EXPECT") {
                // 2. 预期帧匹配动作
                let mut condition = after_expect.trim().to_string();
                let mut timeout = Duration::from_secs(5); // 默认 5 秒超时
                let mut assertions = Vec::new();
                let mut captures = Vec::new();

                if condition.is_empty() {
                    let mut accumulated = String::new();
                    while let Some(next_line) = lines.peek() {
                        let next_trimmed = next_line.trim();
                        if next_trimmed.starts_with("SEND")
                            || next_trimmed.starts_with("EXPECT")
                            || next_trimmed.starts_with("WAIT")
                            || next_trimmed.starts_with("CLOSE")
                            || next_trimmed.starts_with("@timeout")
                            || next_trimmed.starts_with("@assert")
                            || next_trimmed.starts_with("@capture")
                        {
                            break;
                        }
                        accumulated.push_str(lines.next().unwrap());
                        accumulated.push('\n');
                    }
                    condition = accumulated.trim().to_string();
                }

                // 循环读取紧随 EXPECT 其后的局部指令 (@timeout, @assert, @capture)
                while let Some(next_line) = lines.peek() {
                    let next_trimmed = next_line.trim();
                    if let Some(after_timeout) = next_trimmed.strip_prefix("@timeout") {
                        let _ = lines.next(); // 消费这一行
                        let content = after_timeout.trim();
                        let content = content.trim_start_matches('=').trim();
                        if let Ok(d) = crate::parser::metadata::parse_duration(content) {
                            timeout = d;
                        } else if let Ok(ms) = content.parse::<u64>() {
                            timeout = Duration::from_millis(ms);
                        }
                    } else if let Some(after_assert) = next_trimmed.strip_prefix("@assert") {
                        let _ = lines.next(); // 消费这一行
                        let content = after_assert.trim();
                        let content = content.trim_start_matches('=').trim().to_string();
                        if !content.is_empty() {
                            assertions.push(content);
                        }
                    } else if let Some(after_capture) = next_trimmed.strip_prefix("@capture") {
                        let _ = lines.next(); // 消费这一行
                        let content = after_capture.trim();
                        let content = content.trim_start_matches('=').trim();
                        let parts: Vec<&str> = content.split_whitespace().collect();
                        if parts.len() >= 3 && parts[1] == "from" {
                            let var_name = parts[0];
                            if let Some(from_idx) = content.find("from") {
                                let source_str = content[from_idx + 4..].trim();
                                if !var_name.is_empty() && !source_str.is_empty() {
                                    captures.push(
                                        crate::variable::capture::VariableCapture::parse(
                                            var_name, source_str,
                                        ),
                                    );
                                }
                            }
                        }
                    } else {
                        break;
                    }
                }

                let (segments, expected_value, operator) = precompile_jsonpath(&condition);
                actions.push(WsAction::Expect {
                    condition,
                    segments,
                    expected_value,
                    operator,
                    timeout,
                    assertions,
                    captures,
                });
            } else if let Some(after_wait) = trimmed.strip_prefix("WAIT") {
                // 3. 阻塞等待动作
                if let Ok(ms) = after_wait.trim().parse::<u64>() {
                    actions.push(WsAction::Wait(Duration::from_millis(ms)));
                } else {
                    return Err(format!("Invalid WAIT duration: '{}'", trimmed));
                }
            } else if trimmed.starts_with("CLOSE") {
                // 4. 主动关闭连接动作
                actions.push(WsAction::Close);
            } else {
                // 5. 隐含的发送帧：若直接是 JSON 结构开头 ( '{' 或 '[' )，自动转换
                if trimmed.starts_with('{') || trimmed.starts_with('[') {
                    let mut accumulated = trimmed.to_string();
                    accumulated.push('\n');
                    while let Some(next_line) = lines.peek() {
                        let next_trimmed = next_line.trim();
                        if next_trimmed.starts_with("SEND")
                            || next_trimmed.starts_with("EXPECT")
                            || next_trimmed.starts_with("WAIT")
                            || next_trimmed.starts_with("CLOSE")
                        {
                            break;
                        }
                        accumulated.push_str(lines.next().unwrap());
                        accumulated.push('\n');
                    }

                    let frame = WsFrame::new(
                        FrameDirection::Outbound,
                        WsFrameType::Text,
                        accumulated.trim().to_string().into_bytes(),
                        0,
                    );
                    actions.push(WsAction::Send(frame));
                }
            }
        }

        Ok(actions)
    }
}

fn precompile_jsonpath(condition: &str) -> (Option<Vec<String>>, Option<String>, Option<String>) {
    let condition = condition.trim();
    if condition.starts_with('{') || condition.starts_with('[') {
        return (None, None, None);
    }

    if condition.starts_with('$') || condition.contains('.') || condition.contains('[') {
        let operators = ["==", "!=", "contains"];
        for op in &operators {
            if let Some(pos) = condition.find(op) {
                let path_part = condition[..pos].trim();
                let val_part = condition[pos + op.len()..].trim();
                let segments = crate::utils::jsonpath::parse_jsonpath_to_segments(path_part);
                if !segments.is_empty() {
                    return (
                        Some(segments),
                        Some(val_part.to_string()),
                        Some(op.to_string()),
                    );
                }
            }
        }
        let segments = crate::utils::jsonpath::parse_jsonpath_to_segments(condition);
        if !segments.is_empty() {
            return (Some(segments), None, None);
        }
    }

    (None, None, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_actions() {
        let body = r#"
        SEND {"action": "ping"}
        EXPECT {"response": "pong"}
        @timeout = 3000
        WAIT 500
        CLOSE
        "#;

        let actions = WsActionParser::parse_body(body).unwrap();
        assert_eq!(actions.len(), 4);

        if let WsAction::Send(frame) = &actions[0] {
            assert_eq!(frame.frame_type, WsFrameType::Text);
            assert_eq!(frame.payload_as_string(), "{\"action\": \"ping\"}");
        } else {
            panic!("Expected Send action");
        }

        if let WsAction::Expect {
            condition, timeout, ..
        } = &actions[1]
        {
            assert_eq!(condition, "{\"response\": \"pong\"}");
            assert_eq!(timeout.as_millis(), 3000);
        } else {
            panic!("Expected Expect action");
        }

        if let WsAction::Wait(duration) = &actions[2] {
            assert_eq!(duration.as_millis(), 500);
        } else {
            panic!("Expected Wait action");
        }

        assert!(matches!(actions[3], WsAction::Close));
    }

    #[test]
    fn test_implicit_send() {
        let body = r#"
        {
            "id": 1,
            "data": "implicit"
        }
        EXPECT {"status": 200}
        "#;
        let actions = WsActionParser::parse_body(body).unwrap();
        assert_eq!(actions.len(), 2);
        if let WsAction::Send(frame) = &actions[0] {
            assert!(frame.payload_as_string().contains("implicit"));
        } else {
            panic!("Expected implicit Send action");
        }
    }

    #[test]
    fn test_parse_duration_units() {
        let body = r#"
        EXPECT {"response": "pong"}
        @timeout = 3s
        EXPECT {"response": "pong2"}
        @timeout 500ms
        "#;
        let actions = WsActionParser::parse_body(body).unwrap();
        assert_eq!(actions.len(), 2);
        if let WsAction::Expect { timeout, .. } = &actions[0] {
            assert_eq!(timeout.as_secs(), 3);
        } else {
            panic!("Expected Expect action");
        }
        if let WsAction::Expect { timeout, .. } = &actions[1] {
            assert_eq!(timeout.as_millis(), 500);
        } else {
            panic!("Expected Expect action");
        }
    }

    #[test]
    fn test_parse_expect_asserts_and_captures() {
        let body = r#"
        EXPECT $.event == "ticker"
        @timeout = 5s
        @assert body.price > 100
        @capture btc_val from body.price
        "#;
        let actions = WsActionParser::parse_body(body).unwrap();
        assert_eq!(actions.len(), 1);
        if let WsAction::Expect {
            condition,
            segments,
            expected_value,
            operator,
            timeout,
            assertions,
            captures,
        } = &actions[0]
        {
            assert_eq!(condition, "$.event == \"ticker\"");
            assert_eq!(segments.as_ref().unwrap(), &vec!["event".to_string()]);
            assert_eq!(expected_value.as_deref(), Some("\"ticker\""));
            assert_eq!(operator.as_deref(), Some("=="));
            assert_eq!(timeout.as_secs(), 5);
            assert_eq!(assertions.len(), 1);
            assert_eq!(assertions[0], "body.price > 100");
            assert_eq!(captures.len(), 1);
            assert_eq!(captures[0].name, "btc_val");
        } else {
            panic!("Expected Expect action");
        }
    }

    #[test]
    fn test_parse_expect_operators() {
        let body = r#"
        EXPECT $.event != "ticker"
        EXPECT $.msg contains "hello"
        EXPECT $.data
        "#;
        let actions = WsActionParser::parse_body(body).unwrap();
        assert_eq!(actions.len(), 3);

        if let WsAction::Expect {
            expected_value,
            operator,
            ..
        } = &actions[0]
        {
            assert_eq!(operator.as_deref(), Some("!="));
            assert_eq!(expected_value.as_deref(), Some("\"ticker\""));
        } else {
            panic!("Expected Expect action");
        }

        if let WsAction::Expect {
            expected_value,
            operator,
            ..
        } = &actions[1]
        {
            assert_eq!(operator.as_deref(), Some("contains"));
            assert_eq!(expected_value.as_deref(), Some("\"hello\""));
        } else {
            panic!("Expected Expect action");
        }

        if let WsAction::Expect {
            expected_value,
            operator,
            ..
        } = &actions[2]
        {
            assert_eq!(*operator, None);
            assert_eq!(*expected_value, None);
        } else {
            panic!("Expected Expect action");
        }
    }
}
