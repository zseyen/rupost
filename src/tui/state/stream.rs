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

        if self.active_panel == Panel::Response {
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

        if self.active_panel == Panel::Response {
            self.response_scroll = 9999;
        }
    }

    pub fn handle_request_finished(
        &mut self,
        result: Result<Response, String>,
        captured_vars: HashMap<String, String>,
        assertions: Vec<AssertionResult>,
    ) {
        self.is_loading = false;
        self.assertions = assertions.clone();

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
