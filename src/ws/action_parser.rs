use crate::ws::frame::{WsAction, WsFrame, WsFrameType, FrameDirection};
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

            if trimmed.starts_with("SEND") {
                // 1. 发送帧动作
                let mut payload_str = trimmed["SEND".len()..].trim().to_string();
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
            } else if trimmed.starts_with("EXPECT") {
                // 2. 预期帧匹配动作
                let mut condition = trimmed["EXPECT".len()..].trim().to_string();
                let mut timeout = Duration::from_secs(5); // 默认 5 秒超时

                if condition.is_empty() {
                    let mut accumulated = String::new();
                    while let Some(next_line) = lines.peek() {
                        let next_trimmed = next_line.trim();
                        if next_trimmed.starts_with("SEND")
                            || next_trimmed.starts_with("EXPECT")
                            || next_trimmed.starts_with("WAIT")
                            || next_trimmed.starts_with("CLOSE")
                            || next_trimmed.starts_with("@timeout")
                        {
                            break;
                        }
                        accumulated.push_str(lines.next().unwrap());
                        accumulated.push('\n');
                    }
                    condition = accumulated.trim().to_string();
                }

                // 检查下一行是否是局部超时的元数据指令 (例如 @timeout = 3000)
                if let Some(next_line) = lines.peek() {
                    let next_trimmed = next_line.trim();
                    if next_trimmed.starts_with("@timeout") {
                        let timeout_line = lines.next().unwrap();
                        if let Some(pos) = timeout_line.find('=') {
                            if let Ok(ms) = timeout_line[pos + 1..].trim().parse::<u64>() {
                                timeout = Duration::from_millis(ms);
                            }
                        }
                    }
                }

                actions.push(WsAction::Expect { condition, timeout });
            } else if trimmed.starts_with("WAIT") {
                // 3. 阻塞等待动作
                if let Ok(ms) = trimmed["WAIT".len()..].trim().parse::<u64>() {
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

        if let WsAction::Expect { condition, timeout } = &actions[1] {
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
}
