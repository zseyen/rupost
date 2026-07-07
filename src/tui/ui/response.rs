use crate::tui::state::{AppState, Panel};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

pub fn render(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let focus = state.active_panel == Panel::Response;
    let border_color = if focus { Color::Cyan } else { Color::DarkGray };

    let is_ws = !state.ws_frames.is_empty()
        || state
            .current_request
            .as_ref()
            .map(|r| r.metadata.websocket)
            .unwrap_or(false);
    let is_sse = !state.sse_stream_body.is_empty();

    let title = if state.is_loading {
        if is_ws {
            " Response Viewer [WS Streaming...] "
        } else if is_sse {
            " Response Viewer [SSE Streaming...] "
        } else {
            " Response Viewer (Loading) "
        }
    } else {
        if is_ws {
            " Response Viewer [WS Completed] "
        } else if is_sse {
            " Response Viewer [SSE Completed] "
        } else {
            " Response Viewer "
        }
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let visible_height = area.height.saturating_sub(2) as usize; // 可视行数
    let max_width = area.width.saturating_sub(2) as usize; // 可视列数

    if is_ws {
        // 1. 获取 WS 原始帧数据源
        let frames_source = if state.is_loading {
            state.ws_frames.clone()
        } else {
            state.load_viewport_sliding_window(state.response_scroll, visible_height);
            state.viewport_cache.iter().cloned().collect::<Vec<_>>()
        };

        // 2. 构造带格式的物理行，并统一进行预折行 (Pre-wrapping)
        let mut visual_lines = Vec::new();
        for f in &frames_source {
            let arrow = if f.is_send { "[→] " } else { "[←] " };
            let arrow_color = if f.is_send {
                Color::Cyan
            } else {
                Color::Yellow
            };

            // 原始日志单行文本
            let text_line = format!("{} {} {}", arrow, f.timestamp, f.content);
            let wrapped = crate::tui::state::VisualLineProcessor::wrap_text(&text_line, max_width);
            for w_line in wrapped {
                // 由于 wrap_text 返回的是纯 Line，我们在首行附加一下箭头的样式
                let raw_str = w_line.to_string();
                if raw_str.starts_with("[→]") {
                    visual_lines.push(Line::from(vec![
                        Span::styled(
                            "[→] ",
                            Style::default()
                                .fg(arrow_color)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("{} ", f.timestamp),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::raw(raw_str[15..].to_string()),
                    ]));
                } else if raw_str.starts_with("[←]") {
                    visual_lines.push(Line::from(vec![
                        Span::styled(
                            "[←] ",
                            Style::default()
                                .fg(arrow_color)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("{} ", f.timestamp),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::raw(raw_str[15..].to_string()),
                    ]));
                } else {
                    visual_lines.push(w_line);
                }
            }
        }

        if visual_lines.is_empty() {
            visual_lines.push(Line::from(Span::styled(
                "WebSocket connected. Listening for frames...",
                Style::default().fg(Color::DarkGray),
            )));
        }

        // 3. 计算精确的切片范围并渲染
        let total_lines = visual_lines.len();
        let max_scroll = total_lines.saturating_sub(visible_height);
        let scroll_y = state.response_scroll.min(max_scroll);

        let end = (scroll_y + visible_height).min(total_lines);
        let sliced = visual_lines[scroll_y..end].to_vec();

        frame.render_widget(Paragraph::new(sliced).block(block), area);
    } else if is_sse {
        // SSE 同样处理
        let raw_body = if state.is_loading {
            state.sse_stream_body.clone()
        } else {
            state.load_viewport_sliding_window(state.response_scroll, visible_height);
            state
                .viewport_cache
                .iter()
                .map(|f| f.content.clone())
                .collect::<Vec<_>>()
                .join("\n")
        };

        let mut visual_lines = Vec::new();
        for line in raw_body.lines() {
            let wrapped = crate::tui::state::VisualLineProcessor::wrap_text(line, max_width);
            if wrapped.is_empty() {
                visual_lines.push(Line::from(""));
            } else {
                for w in wrapped {
                    visual_lines.push(w);
                }
            }
        }

        if visual_lines.is_empty() {
            visual_lines.push(Line::from(Span::styled(
                "SSE streaming connected...",
                Style::default().fg(Color::DarkGray),
            )));
        }

        let total_lines = visual_lines.len();
        let max_scroll = total_lines.saturating_sub(visible_height);
        let scroll_y = state.response_scroll.min(max_scroll);

        let end = (scroll_y + visible_height).min(total_lines);
        let sliced = visual_lines[scroll_y..end].to_vec();

        frame.render_widget(Paragraph::new(sliced).block(block), area);
    } else if state.is_loading {
        frame.render_widget(
            Paragraph::new("[RUNNING] Sending request...")
                .style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )
                .block(block),
            area,
        );
    } else if state.last_response.is_some() {
        // 4. 普通 HTTP 响应，如果 response_visual_lines 为空或发生变化，进行重建折行
        if state.response_visual_lines.is_empty() {
            rebuild_http_visual_lines(state, max_width);
        }

        let total_lines = state.response_visual_lines.len();
        let max_scroll = total_lines.saturating_sub(visible_height);
        let scroll_y = state.response_scroll.min(max_scroll);

        let end = (scroll_y + visible_height).min(total_lines);
        let sliced = state.response_visual_lines[scroll_y..end].to_vec();

        frame.render_widget(Paragraph::new(sliced).block(block), area);
    } else {
        frame.render_widget(
            Paragraph::new("No response data. Trigger execution via Ctrl+Enter.")
                .style(Style::default().fg(Color::DarkGray))
                .block(block),
            area,
        );
    }
}

fn rebuild_http_visual_lines(state: &mut AppState, max_width: usize) {
    if let Some(ref resp) = state.last_response {
        let mut raw_lines = Vec::new();
        let status_color = if resp.is_success() {
            Color::Green
        } else {
            Color::Red
        };

        let has_failures = !resp.is_success() || state.assertions.iter().any(|a| !a.passed);
        let status_text = if has_failures {
            "[FAILED]"
        } else {
            "[SUCCESS]"
        };
        let status_text_color = if has_failures {
            Color::Red
        } else {
            Color::Green
        };

        // 拼接任务诊断状态栏
        raw_lines.push(Line::from(vec![
            Span::styled(
                format!("{} ", status_text),
                Style::default()
                    .fg(status_text_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "Status: {} {} ",
                    resp.status.code(),
                    resp.status.reason_phrase()
                ),
                Style::default().fg(status_color),
            ),
        ]));

        let mut time_size_spans = vec![
            Span::styled("Time: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{}ms  ", resp.duration.as_millis())),
            Span::styled("Size: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{} bytes  ", resp.body.len())),
        ];

        // 拼接断言进度统计
        let total_assertions = state.assertions.len();
        if total_assertions > 0 {
            let passed_assertions = state.assertions.iter().filter(|a| a.passed).count();
            let failed_assertions = total_assertions - passed_assertions;
            let assertions_color = if failed_assertions > 0 {
                Color::Red
            } else {
                Color::Green
            };
            time_size_spans.push(Span::styled(
                format!(
                    "Assertions: {} passed, {} failed",
                    passed_assertions, failed_assertions
                ),
                Style::default().fg(assertions_color),
            ));
        }
        raw_lines.push(Line::from(time_size_spans));
        raw_lines.push(Line::from(""));
        raw_lines.push(Line::from(Span::styled(
            "Body:",
            Style::default().fg(Color::Yellow),
        )));

        // 拼接响应 Body 各行，若行数超限 1000 则进行截断保护
        let max_body_lines = 1000;
        let mut body_lines = Vec::new();
        let mut is_truncated = false;
        let mut total_body_lines = 0;

        for line in resp.body.lines() {
            total_body_lines += 1;
            if body_lines.len() < max_body_lines {
                body_lines.push(line.to_string());
            } else {
                is_truncated = true;
            }
        }

        if is_truncated {
            raw_lines.push(Line::from(Span::styled(
                format!(
                    " [WARNING: Response truncated from {} to 1000 lines. View full log in CLI] ",
                    total_body_lines
                ),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
        }

        for line in body_lines {
            raw_lines.push(Line::from(line));
        }

        // 调用 pre-wrapper 折行处理器
        let mut wrapped = Vec::new();
        for line in raw_lines {
            let line_str = line.to_string();
            // 注意：若是一行空行，需保留
            if line_str.is_empty() {
                wrapped.push(Line::from(""));
                continue;
            }
            let wrapped_sub =
                crate::tui::state::VisualLineProcessor::wrap_text(&line_str, max_width);
            for w in wrapped_sub {
                wrapped.push(w);
            }
        }
        state.response_visual_lines = wrapped;
    }
}
