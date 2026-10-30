//! 流式事件域：spinner、SSE/WS 流入帧、请求完成事件的 AppState 归约。

use super::*;

impl AppState {
    pub fn get_spinner_char(&self) -> &'static str {
        let spinner_chars = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        spinner_chars[(self.loading_tick / 2) % spinner_chars.len()]
    }

    pub fn handle_stream_chunk(&mut self, chunk: String) {
        self.sse_stream_body.push_str(&chunk);
        self.total_log_lines += chunk.matches('\n').count();

        let max_width = self.current_response_width;
        let body_ref = &self.sse_stream_body;

        if let Some(last_newline_idx) = body_ref[self.sse_last_processed_pos..].rfind('\n') {
            let actual_newline_idx = self.sse_last_processed_pos + last_newline_idx;
            let completed_text = &body_ref[self.sse_last_processed_pos..=actual_newline_idx];
            for line in completed_text.lines() {
                let wrapped = VisualLineProcessor::wrap_text(line, max_width);
                if wrapped.is_empty() {
                    self.sse_visual_lines.push(ratatui::text::Line::from(""));
                } else {
                    self.sse_visual_lines.extend(wrapped);
                }
            }
            self.sse_last_processed_pos = actual_newline_idx + 1;
        }

        if self.active_panel == Panel::Response && self.search_query.is_empty() {
            // 搜索态暂停自动吸底，避免跳转被覆盖
            self.response_scroll = 9999;
        }
    }

    pub fn handle_ws_frame(&mut self, is_send: bool, content: String) {
        self.total_log_lines += 1;
        let now = chrono::Local::now().format("%H:%M:%S").to_string();
        let arrow = if is_send { "->" } else { "<-" };
        let text_line = format!("{} {} {}", arrow, now, content);
        let max_width = self.current_response_width;
        let wrapped = VisualLineProcessor::wrap_text(&text_line, max_width);

        self.ws_frames.push(WsFrameRecord {
            is_send,
            timestamp: now,
            content,
        });

        if self.ws_frames.len() > 100 {
            self.ws_frames.remove(0);
            self.ws_visual_lines.clear();
            for f in &self.ws_frames {
                let a = if f.is_send { "->" } else { "<-" };
                let t = format!("{} {} {}", a, f.timestamp, f.content);
                let w = VisualLineProcessor::wrap_text(&t, max_width);
                self.ws_visual_lines.extend(w);
            }
        } else {
            self.ws_visual_lines.extend(wrapped);
        }

        if self.active_panel == Panel::Response && self.search_query.is_empty() {
            // 搜索态暂停自动吸底，避免跳转被覆盖
            self.response_scroll = 9999;
        }
    }

    pub fn handle_request_finished(
        &mut self,
        result: Result<Response, String>,
        captured_vars: HashMap<String, String>,
        assertions: Vec<AssertionResult>,
        warnings: Vec<String>,
    ) {
        self.is_loading = false;
        self.assertions = assertions.clone();
        self.warnings = warnings;

        let is_success = result.is_ok() && assertions.iter().all(|a| a.passed);
        let exec_state = if is_success {
            FileExecState::Success
        } else {
            FileExecState::Failed
        };

        if let Some(ref path) = self.editor_file_path {
            if self.active_sidebar_tab == SidebarTab::Files {
                self.file_execution_states.insert(path.clone(), exec_state);
            }
        }

        match result {
            Ok(resp) => {
                self.last_response = Some(resp);
                self.variables.extend(captured_vars);
            }
            Err(e) => {
                self.last_response = Some(Response::error(e));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_chunk_completes_lines_into_visual_cache() {
        let mut state = AppState::new();
        state.current_response_width = 40;

        state.handle_stream_chunk("line 1\nline 2\n".to_string());

        assert_eq!(state.total_log_lines, 2);
        assert_eq!(state.sse_last_processed_pos, 14);
        let rendered: Vec<String> = state
            .sse_visual_lines
            .iter()
            .map(|l| l.to_string())
            .collect();
        assert_eq!(rendered, vec!["line 1", "line 2"]);
    }

    #[test]
    fn test_stream_chunk_partial_line_stays_active() {
        let mut state = AppState::new();

        state.handle_stream_chunk("event: me".to_string());

        // 未闭合的活动行不得提前折入缓存，处理位点不得推进
        assert!(state.sse_visual_lines.is_empty());
        assert_eq!(state.sse_last_processed_pos, 0);
        assert_eq!(state.total_log_lines, 0);
        assert_eq!(state.sse_stream_body, "event: me");
    }

    #[test]
    fn test_stream_chunk_incremental_across_chunks() {
        let mut state = AppState::new();
        state.current_response_width = 40;

        state.handle_stream_chunk("hello\nwor".to_string());
        assert_eq!(state.sse_last_processed_pos, 6);
        assert_eq!(state.sse_visual_lines.len(), 1);

        state.handle_stream_chunk("ld\nnext\n".to_string());

        let rendered: Vec<String> = state
            .sse_visual_lines
            .iter()
            .map(|l| l.to_string())
            .collect();
        assert_eq!(rendered, vec!["hello", "world", "next"]);
        // "hello\nworld\nnext\n" 末个 '\n' 在 16，位点指向其后
        assert_eq!(state.sse_last_processed_pos, 17);
        assert_eq!(state.total_log_lines, 3);
    }

    #[test]
    fn test_stream_chunk_wide_char_wrap_uses_display_width() {
        let mut state = AppState::new();
        state.current_response_width = 6; // 3 个 CJK 全宽字符

        state.handle_stream_chunk("你好世界，你好。\n".to_string());

        let rendered: Vec<String> = state
            .sse_visual_lines
            .iter()
            .map(|l| l.to_string())
            .collect();
        assert_eq!(rendered, vec!["你好世", "界，你", "好。"]);
        // 8 个 CJK 字符 = 24 字节 + 结尾换行，位点指向最后一个 '\n' 之后
        assert_eq!(state.sse_last_processed_pos, 25);
    }

    #[test]
    fn test_stream_chunk_autoscroll_only_when_response_focused() {
        let mut state = AppState::new();
        state.active_panel = Panel::Response;
        state.response_scroll = 0;

        state.handle_stream_chunk("tick\n".to_string());
        assert_eq!(state.response_scroll, 9999);

        state.active_panel = Panel::Editor;
        state.response_scroll = 3;
        state.handle_stream_chunk("tick\n".to_string());
        assert_eq!(state.response_scroll, 3);
    }

    #[test]
    fn test_ws_frame_builds_visual_line_with_direction_prefix() {
        let mut state = AppState::new();

        state.handle_ws_frame(true, "PING".to_string());
        state.handle_ws_frame(false, "PONG".to_string());

        assert_eq!(state.total_log_lines, 2);
        assert_eq!(state.ws_visual_lines.len(), 2);
        let first = state.ws_visual_lines[0].to_string();
        let second = state.ws_visual_lines[1].to_string();
        assert!(
            first.starts_with("-> ") && first.ends_with(" PING"),
            "send frame must render with -> prefix: {first}"
        );
        assert!(
            second.starts_with("<- ") && second.ends_with(" PONG"),
            "recv frame must render with <- prefix: {second}"
        );
        assert_eq!(
            state.ws_frames[0].timestamp,
            first.split(' ').nth(1).unwrap()
        );
    }

    #[test]
    fn test_ws_frame_overflow_rebuilds_visual_lines_from_records() {
        let mut state = AppState::new();

        for i in 0..101 {
            state.handle_ws_frame(true, format!("Frame {i}"));
        }

        assert_eq!(state.ws_frames.len(), 100);
        assert_eq!(state.ws_frames[0].content, "Frame 1");
        // 溢出后视觉行必须与存留帧记录一一对应（每帧单行）
        assert_eq!(state.ws_visual_lines.len(), 100);
        assert!(state.ws_visual_lines[0].to_string().contains("Frame 1"));
        assert!(!state.ws_visual_lines[0].to_string().contains("Frame 0"));
        assert!(state.ws_visual_lines[99].to_string().contains("Frame 100"));
    }

    #[test]
    fn test_request_finished_error_lands_as_error_response() {
        let mut state = AppState::new();
        state.is_loading = true;
        state.editor_file_path = Some("examples/fail.http".to_string());

        state.handle_request_finished(Err("boom".to_string()), HashMap::new(), vec![], vec![]);

        assert!(!state.is_loading);
        let resp = state
            .last_response
            .expect("error response must be materialized");
        assert!(!resp.is_success());
        assert_eq!(resp.body, "boom");
        assert_eq!(
            state.file_execution_states.get("examples/fail.http"),
            Some(&FileExecState::Failed)
        );
    }

    #[test]
    fn test_request_finished_success_marks_file_and_merges_vars() {
        let mut state = AppState::new();
        state.is_loading = true;
        state.editor_file_path = Some("examples/ok.http".to_string());

        let mut captured = HashMap::new();
        captured.insert("token".to_string(), "t-1".to_string());
        let resp = Response::new(
            200,
            reqwest::header::HeaderMap::new(),
            "OK".to_string(),
            std::time::Duration::from_millis(1),
            std::time::Duration::from_millis(1),
            std::time::Duration::from_millis(1),
        )
        .unwrap();

        state.handle_request_finished(Ok(resp), captured, vec![], vec![]);

        assert_eq!(
            state.file_execution_states.get("examples/ok.http"),
            Some(&FileExecState::Success)
        );
        assert_eq!(state.variables.get("token").unwrap(), "t-1");
    }

    #[test]
    fn test_request_finished_stores_warnings_until_next_send() {
        use crate::parser::ParsedRequest;
        use crate::tui::event::Action;

        let mut state = AppState::new();
        state.is_loading = true;

        let resp = Response::new(
            200,
            reqwest::header::HeaderMap::new(),
            "OK".to_string(),
            std::time::Duration::from_millis(1),
            std::time::Duration::from_millis(1),
            std::time::Duration::from_millis(1),
        )
        .unwrap();

        state.handle_request_finished(
            Ok(resp),
            HashMap::new(),
            vec![],
            vec!["@capture failed: body.id".to_string()],
        );
        assert_eq!(state.warnings.len(), 1);
        assert!(state.warnings[0].contains("@capture"));

        // 新一次请求启动必须清空旧告警，避免跨请求残留
        let req = ParsedRequest {
            method: Some("GET".to_string()),
            url: "http://localhost".to_string(),
            headers: vec![],
            body: None,
            metadata: Default::default(),
            line_number: 1,
            base_path: None,
        };
        state.update(Action::SendRequest(Box::new(req)));
        assert!(state.warnings.is_empty());

        // Err 分支同样透传告警（警告与失败正交：错误响应也可伴随 capture/变量告警）
        state.handle_request_finished(
            Err("connection refused".to_string()),
            HashMap::new(),
            vec![],
            vec!["undefined variable {{api_key}}".to_string()],
        );
        assert_eq!(state.warnings.len(), 1);
        let err_resp = state
            .last_response
            .expect("error response must be materialized");
        assert!(!err_resp.is_success());
    }

    /// R4 守护：SSE 流式吸底在搜索态暂停（与 WS 变体对称）
    #[test]
    fn test_sse_autoscroll_paused_while_searching() {
        let mut state = AppState::new();
        state.active_panel = Panel::Response;

        // 无 query：自动吸底
        state.handle_stream_chunk("tick\n".to_string());
        assert_eq!(state.response_scroll, 9999);

        // 有 query：暂停吸底，滚动保持
        state.response_scroll = 3;
        state.search_query = "tick".to_string();
        state.handle_stream_chunk("tick\n".to_string());
        assert_eq!(state.response_scroll, 3);
    }
}
