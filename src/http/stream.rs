/// Server-Sent Events (SSE) 事件对象
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    /// 事件 ID (@id)
    pub id: Option<String>,
    /// 事件类型 (@event)
    pub event: Option<String>,
    /// 数据负载 (@data)
    pub data: String,
}

/// 用于解析 Server-Sent Events 流的状态机解析器
pub struct SseParser {
    buffer: String,
    current_id: Option<String>,
    current_event: Option<String>,
    current_data: Vec<String>,
}

impl SseParser {
    /// 创建一个新的 SSE 解析器
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            current_id: None,
            current_event: None,
            current_data: Vec::new(),
        }
    }

    /// 向解析器喂入网络字节数据块，返回解析出的完整 SSE 事件列表
    pub fn feed(&mut self, chunk: &str) -> Vec<SseEvent> {
        self.buffer.push_str(chunk);
        let mut events = Vec::new();

        while let Some(pos) = self.buffer.find('\n') {
            let mut line = self.buffer.drain(..=pos).collect::<String>();
            // 剥除换行符
            if line.ends_with('\n') {
                line.pop();
            }
            if line.ends_with('\r') {
                line.pop();
            }

            if line.is_empty() {
                // 空行代表当前事件结束，开始分发事件
                if !self.current_data.is_empty() || self.current_event.is_some() || self.current_id.is_some() {
                    events.push(SseEvent {
                        id: self.current_id.take(),
                        event: self.current_event.take(),
                        data: self.current_data.join("\n"),
                    });
                    self.current_data.clear();
                }
            } else if line.starts_with(':') {
                // 忽略注释行（如心跳保持包）
            } else {
                let (key, value) = match line.split_once(':') {
                    Some((k, v)) => {
                        let trimmed_v = if v.starts_with(' ') {
                            &v[1..]
                        } else {
                            v
                        };
                        (k, trimmed_v)
                    }
                    None => (line.as_str(), ""),
                };

                match key {
                    "event" => self.current_event = Some(value.to_string()),
                    "data" => self.current_data.push(value.to_string()),
                    "id" => self.current_id = Some(value.to_string()),
                    _ => {} // 忽略其他字段（如 retry）
                }
            }
        }

        events
    }

    /// 强制刷新当前缓冲区，分发可能未完成的最后一个事件（通常在流结束时调用）
    pub fn flush(&mut self) -> Option<SseEvent> {
        if !self.buffer.is_empty() {
            let line = std::mem::take(&mut self.buffer);
            let line = line.trim();
            if !line.is_empty() && !line.starts_with(':') {
                let (key, value) = match line.split_once(':') {
                    Some((k, v)) => {
                        let trimmed_v = if v.starts_with(' ') {
                            &v[1..]
                        } else {
                            v
                        };
                        (k, trimmed_v)
                    }
                    None => (line, ""),
                };

                match key {
                    "event" => self.current_event = Some(value.to_string()),
                    "data" => self.current_data.push(value.to_string()),
                    "id" => self.current_id = Some(value.to_string()),
                    _ => {}
                }
            }
        }

        if !self.current_data.is_empty() || self.current_event.is_some() || self.current_id.is_some() {
            let event = SseEvent {
                id: self.current_id.take(),
                event: self.current_event.take(),
                data: self.current_data.join("\n"),
            };
            self.current_data.clear();
            Some(event)
        } else {
            None
        }
    }
}

impl Default for SseParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sse_parser_basic_frame() {
        let mut parser = SseParser::new();
        let payload = "id: 1\nevent: message\ndata: {\"count\": 42}\n\n";
        let events = parser.feed(payload);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, Some("1".to_string()));
        assert_eq!(events[0].event, Some("message".to_string()));
        assert_eq!(events[0].data, "{\"count\": 42}");
    }

    #[test]
    fn test_sse_parser_multiline_data() {
        let mut parser = SseParser::new();
        let payload = "data: line one\ndata: line two\n\n";
        let events = parser.feed(payload);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "line one\nline two");
    }

    #[test]
    fn test_sse_parser_fragmented_chunks() {
        let mut parser = SseParser::new();
        
        let chunk1 = "id: 10\nevent:";
        let events1 = parser.feed(chunk1);
        assert!(events1.is_empty());

        let chunk2 = " update\ndata: hello";
        let events2 = parser.feed(chunk2);
        assert!(events2.is_empty());

        let chunk3 = " world\n\n";
        let events3 = parser.feed(chunk3);
        assert_eq!(events3.len(), 1);
        assert_eq!(events3[0].id, Some("10".to_string()));
        assert_eq!(events3[0].event, Some("update".to_string()));
        assert_eq!(events3[0].data, "hello world");
    }

    #[test]
    fn test_sse_parser_filter_ping_comments() {
        let mut parser = SseParser::new();
        let payload = ": keepalive heartbeat ping\nid: 2\ndata: actual content\n\n";
        let events = parser.feed(payload);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, Some("2".to_string()));
        assert_eq!(events[0].data, "actual content");
    }

    #[test]
    fn test_sse_parser_flush() {
        let mut parser = SseParser::new();
        let payload = "id: 99\ndata: partial end without double newline";
        let events = parser.feed(payload);
        assert!(events.is_empty());

        let flushed = parser.flush();
        assert!(flushed.is_some());
        let event = flushed.unwrap();
        assert_eq!(event.id, Some("99".to_string()));
        assert_eq!(event.data, "partial end without double newline");
    }
}
