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

/// 解析器内部缓冲上限：无换行残片与单事件 data 累积各 1MB。
/// 超限只告警并截断（截断状态在事件边界复位），防御恶意流撑爆内存。
const MAX_BUFFERED_BYTES: usize = 1024 * 1024;

/// 用于解析 Server-Sent Events 流的状态机解析器
pub struct SseParser {
    buffer: String,
    current_id: Option<String>,
    current_event: Option<String>,
    current_data: Vec<String>,
    /// 单事件 data 累积超限后置位：丢弃后续 data 行，直到事件边界（空行）复位
    data_overflow: bool,
    /// 残片 buffer 超限告警去重：持续溢出只告警一次，行处理完毕后复位
    buffer_overflow_warned: bool,
}

impl SseParser {
    /// 创建一个新的 SSE 解析器
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            current_id: None,
            current_event: None,
            current_data: Vec::new(),
            data_overflow: false,
            buffer_overflow_warned: false,
        }
    }

    /// 追加一行 data 载荷；超过单事件上限后丢弃本事件剩余的 data 行（告警一次）
    fn push_data_line(&mut self, value: &str) {
        let accumulated: usize = self.current_data.iter().map(|s| s.len() + 1).sum();
        if self.data_overflow || accumulated.saturating_add(value.len()) > MAX_BUFFERED_BYTES {
            if !self.data_overflow {
                tracing::warn!(
                    "SSE event data exceeded {} bytes; truncating the current event",
                    MAX_BUFFERED_BYTES
                );
                self.data_overflow = true;
            }
            return;
        }
        self.current_data.push(value.to_string());
    }

    /// 向解析器喂入网络字节数据块，返回解析出的完整 SSE 事件列表
    pub fn feed(&mut self, chunk: &str) -> Vec<SseEvent> {
        self.buffer.push_str(chunk);
        // 无换行残片超限：截断保留尾部（最新数据），从字符边界切
        if self.buffer.len() > MAX_BUFFERED_BYTES {
            if !self.buffer_overflow_warned {
                tracing::warn!(
                    "SSE line buffer exceeded {} bytes without a newline; truncating",
                    MAX_BUFFERED_BYTES
                );
                self.buffer_overflow_warned = true;
            }
            let mut cut = self.buffer.len() - MAX_BUFFERED_BYTES;
            while !self.buffer.is_char_boundary(cut) {
                cut += 1;
            }
            self.buffer.drain(..cut);
        }
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
                // 空行代表当前事件结束（事件边界：data 超限截断状态在此复位）。
                // WHATWG 规范：data 缓冲区为空时不派发（否则 `event: ping` 类心跳帧
                // 会产生空事件、污染 sse_max_events 计数与逐帧断言），且此时只重置
                // event type；id 缓冲区留存（lastEventId 跨帧语义），随下一次真正
                // 派发的事件带走。
                self.data_overflow = false;
                if self.current_data.is_empty() {
                    self.current_event = None;
                } else {
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
                    Some((k, v)) => (k, v.strip_prefix(' ').unwrap_or(v)),
                    None => (line.as_str(), ""),
                };

                match key {
                    "event" => self.current_event = Some(value.to_string()),
                    "data" => self.push_data_line(value),
                    "id" => self.current_id = Some(value.to_string()),
                    _ => {} // 忽略其他字段（如 retry）
                }
            }
        }

        if self.buffer.is_empty() {
            self.buffer_overflow_warned = false;
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
                    Some((k, v)) => (k, v.strip_prefix(' ').unwrap_or(v)),
                    None => (line, ""),
                };

                match key {
                    "event" => self.current_event = Some(value.to_string()),
                    "data" => self.push_data_line(value),
                    "id" => self.current_id = Some(value.to_string()),
                    _ => {}
                }
            }
        }

        // 与 feed() 的派发规则一致：data 缓冲区为空则不派发（只重置 event type，id 留存）
        if self.current_data.is_empty() {
            self.current_event = None;
            None
        } else {
            let event = SseEvent {
                id: self.current_id.take(),
                event: self.current_event.take(),
                data: self.current_data.join("\n"),
            };
            self.current_data.clear();
            Some(event)
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
    fn test_empty_data_frames_are_not_dispatched() {
        // 回归（WHATWG 对齐）：无 data 行的帧此前照发——`event: ping` 类心跳帧
        // 会产生空事件、污染 sse_max_events 计数与逐帧断言
        let mut parser = SseParser::new();
        assert!(parser.feed("event: ping\n\n").is_empty());

        // event type 被重置：下一帧不再带上 ping
        let events = parser.feed("data: real\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, None);
        assert_eq!(events[0].data, "real");
    }

    #[test]
    fn test_id_persists_across_undispatched_frame() {
        // id（lastEventId）跨帧留存：未派发帧携带的 id 随下一个真正派发的事件带走
        let mut parser = SseParser::new();
        assert!(parser.feed("id: 7\n\n").is_empty());
        let events = parser.feed("data: after\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, Some("7".to_string()));
    }

    #[test]
    fn test_empty_data_value_still_dispatches() {
        // `data:` 空值行仍然向缓冲区 push 空串（Vec 非空）→ 照常派发空串数据
        let mut parser = SseParser::new();
        let events = parser.feed("data:\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "");
    }

    #[test]
    fn test_flush_respects_empty_data_rule() {
        let mut parser = SseParser::new();
        parser.feed("event: lonely"); // 流结束，无 data 行
        assert!(parser.flush().is_none());
    }

    #[test]
    fn test_line_buffer_is_capped() {
        // 回归：恶意流发送无换行的超长数据时，残片 buffer 必须有上限（此前无界增长）
        let mut parser = SseParser::new();
        parser.feed(&"x".repeat(MAX_BUFFERED_BYTES * 2));
        assert!(
            parser.buffer.len() <= MAX_BUFFERED_BYTES,
            "buffer 应被截断到上限内，实际 {}",
            parser.buffer.len()
        );
    }

    #[test]
    fn test_event_data_accumulation_is_capped_and_recovers() {
        // 回归：大量 `data:` 行不派发时此前在 current_data 无界累积。
        // 超限后丢弃本事件剩余 data 行（告警），事件边界复位后恢复正常。
        let mut parser = SseParser::new();
        let oversized_line = format!("data: {}\n", "y".repeat(MAX_BUFFERED_BYTES + 1));
        parser.feed(&oversized_line);
        parser.feed("\n"); // 事件边界：溢出标记复位
        assert!(parser.current_data.is_empty());

        // 后续正常事件不受影响
        let events = parser.feed("data: recovered\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "recovered");
    }

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
