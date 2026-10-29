use crate::tui::state::{AppState, DetailTab, Panel};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs},
};

pub fn render(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let t = state.palette();
    let focus = state.active_panel == Panel::Response;
    let border_color = if focus { t.info } else { t.dim };

    // 1. 垂直划分选项卡 Tab 栏和内容显示区
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // 顶部 Tab 栏
            Constraint::Min(0),    // 主要内容区
        ])
        .split(area);
    let tab_area = chunks[0];
    let content_area = chunks[1];

    // 2. 渲染详情子选项卡
    let tab_titles = vec!["[1] Response", "[2] Headers", "[3] Request"];
    let active_idx = match state.active_detail_tab {
        DetailTab::Response => 0,
        DetailTab::Headers => 1,
        DetailTab::Request => 2,
    };
    let tabs = Tabs::new(tab_titles)
        .select(active_idx)
        .style(Style::default().fg(t.dim))
        .highlight_style(Style::default().fg(t.info).add_modifier(Modifier::BOLD));
    frame.render_widget(tabs, tab_area);

    let is_ws = !state.ws_frames.is_empty()
        || state
            .current_request
            .as_ref()
            .map(|r| r.metadata.websocket)
            .unwrap_or(false);
    let is_sse = !state.sse_stream_body.is_empty();

    let title = if state.is_loading {
        let spinner_char = state.get_spinner_char();
        if is_ws {
            format!(" Response Viewer [WS Streaming {}] ", spinner_char)
        } else if is_sse {
            format!(" Response Viewer [SSE Streaming {}] ", spinner_char)
        } else {
            format!(" Response Viewer [Loading {}] ", spinner_char)
        }
    } else {
        if is_ws {
            " Response Viewer [WS Completed] ".to_string()
        } else if is_sse {
            " Response Viewer [SSE Completed] ".to_string()
        } else {
            " Response Viewer ".to_string()
        }
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let visible_height = content_area.height.saturating_sub(2) as usize; // 可视行数
    let max_width = content_area.width.saturating_sub(2) as usize; // 可视列数

    let use_streaming = (is_ws || is_sse) && state.active_detail_tab == DetailTab::Response;

    if use_streaming && is_ws {
        let visual_lines = if state.is_loading {
            state.ws_visual_lines.clone()
        } else {
            // 获取 WS 原始帧数据源
            state.load_viewport_sliding_window(state.response_scroll, visible_height);
            let mut lines = Vec::new();
            for f in &state.viewport_cache {
                let arrow = if f.is_send { "[→] " } else { "[←] " };
                let arrow_color = if f.is_send { t.info } else { t.accent };
                let text_line = format!("{} {} {}", arrow, f.timestamp, f.content);
                let wrapped =
                    crate::tui::state::VisualLineProcessor::wrap_text(&text_line, max_width);
                for w_line in wrapped {
                    let raw_str = w_line.to_string();
                    if raw_str.starts_with("[→]") {
                        lines.push(Line::from(vec![
                            Span::styled(
                                "[→] ",
                                Style::default()
                                    .fg(arrow_color)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(format!("{} ", f.timestamp), Style::default().fg(t.dim)),
                            Span::raw(raw_str[15..].to_string()),
                        ]));
                    } else if raw_str.starts_with("[←]") {
                        lines.push(Line::from(vec![
                            Span::styled(
                                "[←] ",
                                Style::default()
                                    .fg(arrow_color)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(format!("{} ", f.timestamp), Style::default().fg(t.dim)),
                            Span::raw(raw_str[15..].to_string()),
                        ]));
                    } else {
                        lines.push(w_line);
                    }
                }
            }
            lines
        };

        let mut visual_lines_final = visual_lines;
        if visual_lines_final.is_empty() {
            visual_lines_final.push(Line::from(Span::styled(
                "WebSocket connected. Listening for frames...",
                Style::default().fg(t.dim),
            )));
        }

        // 3. 计算精确的切片范围并渲染
        let total_lines = visual_lines_final.len();
        let max_scroll = total_lines.saturating_sub(visible_height);
        let scroll_y = state.response_scroll.min(max_scroll);

        let end = (scroll_y + visible_height).min(total_lines);
        let sliced = visual_lines_final[scroll_y..end].to_vec();

        frame.render_widget(Paragraph::new(sliced).block(block), area);
    } else if is_sse {
        let visual_lines = if state.is_loading {
            // 1. 若因 Resize 导致缓存被清空，执行一次性的惰性全量重构
            if state.sse_visual_lines.is_empty() && !state.sse_stream_body.is_empty() {
                let body = &state.sse_stream_body;
                if let Some(last_newline_idx) = body.rfind('\n') {
                    let completed_text = &body[..=last_newline_idx];
                    for line in completed_text.lines() {
                        let wrapped =
                            crate::tui::state::VisualLineProcessor::wrap_text(line, max_width);
                        for w in wrapped {
                            state.sse_visual_lines.push(w);
                        }
                    }
                    state.sse_last_processed_pos = last_newline_idx + 1;
                }
            }

            // 2. 取出已缓存行，并动态折行最后一行的未闭合活动行 (active line)
            let active_line = &state.sse_stream_body[state.sse_last_processed_pos..];
            let mut lines = state.sse_visual_lines.clone();
            if !active_line.is_empty() {
                let wrapped_active =
                    crate::tui::state::VisualLineProcessor::wrap_text(active_line, max_width);
                for w in wrapped_active {
                    lines.push(w);
                }
            }
            lines
        } else {
            state.load_viewport_sliding_window(state.response_scroll, visible_height);
            let raw_body = state
                .viewport_cache
                .iter()
                .map(|f| f.content.clone())
                .collect::<Vec<_>>()
                .join("\n");

            let mut lines = Vec::new();
            for line in raw_body.lines() {
                let wrapped = crate::tui::state::VisualLineProcessor::wrap_text(line, max_width);
                if wrapped.is_empty() {
                    lines.push(Line::from(""));
                } else {
                    for w in wrapped {
                        lines.push(w);
                    }
                }
            }
            lines
        };

        let mut visual_lines_final = visual_lines;
        if visual_lines_final.is_empty() {
            visual_lines_final.push(Line::from(Span::styled(
                "SSE streaming connected...",
                Style::default().fg(t.dim),
            )));
        }

        let total_lines = visual_lines_final.len();
        let max_scroll = total_lines.saturating_sub(visible_height);
        let scroll_y = state.response_scroll.min(max_scroll);

        let end = (scroll_y + visible_height).min(total_lines);
        let sliced = visual_lines_final[scroll_y..end].to_vec();

        frame.render_widget(Paragraph::new(sliced).block(block), area);
    } else if state.is_loading {
        frame.render_widget(
            Paragraph::new(format!(
                "[RUNNING] Sending request... {}",
                state.get_spinner_char()
            ))
            .style(Style::default().fg(t.accent).add_modifier(Modifier::BOLD))
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

        frame.render_widget(Paragraph::new(sliced).block(block), content_area);
    } else {
        frame.render_widget(
            Paragraph::new("No response data. Trigger execution via Ctrl+Enter.")
                .style(Style::default().fg(t.dim))
                .block(block),
            content_area,
        );
    }
}

fn rebuild_http_visual_lines(state: &mut AppState, max_width: usize) {
    let t = state.palette();
    use crate::tui::state::DetailTab;
    if let Some(ref resp) = state.last_response {
        let mut raw_lines = Vec::new();

        match state.active_detail_tab {
            DetailTab::Response => {
                let status_color = if resp.is_success() { t.ok } else { t.err };

                let has_failures = !resp.is_success() || state.assertions.iter().any(|a| !a.passed);
                let status_text = if has_failures {
                    "[FAILED]"
                } else {
                    "[SUCCESS]"
                };
                let status_text_color = if has_failures { t.err } else { t.ok };

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
                    Span::styled("Time: ", Style::default().fg(t.dim)),
                    Span::raw(format!("{}ms  ", resp.duration.as_millis())),
                    Span::styled("Size: ", Style::default().fg(t.dim)),
                    Span::raw(format!("{} bytes  ", resp.body.len())),
                ];

                // 拼接断言进度统计
                let total_assertions = state.assertions.len();
                if total_assertions > 0 {
                    let passed_assertions = state.assertions.iter().filter(|a| a.passed).count();
                    let failed_assertions = total_assertions - passed_assertions;
                    let assertions_color = if failed_assertions > 0 { t.err } else { t.ok };

                    time_size_spans.push(Span::styled(
                        format!(
                            "Assertions: {} passed, {} failed",
                            passed_assertions, failed_assertions
                        ),
                        Style::default().fg(assertions_color),
                    ));
                }
                raw_lines.push(Line::from(time_size_spans));

                // 非致命告警必须可见，避免静默假通过
                if !state.warnings.is_empty() {
                    raw_lines.push(Line::from(Span::styled(
                        format!("Warnings: {}", state.warnings.len()),
                        Style::default()
                            .fg(t.accent)
                            .add_modifier(Modifier::BOLD),
                    )));
                    for w in &state.warnings {
                        raw_lines.push(Line::from(Span::styled(
                            format!("⚠ {w}"),
                            Style::default().fg(t.accent),
                        )));
                    }
                }

                raw_lines.push(Line::from(""));
                raw_lines.push(Line::from(Span::styled(
                    "Body:",
                    Style::default().fg(t.accent),
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
                            .fg(t.accent)
                            .add_modifier(Modifier::BOLD),
                    )));
                }

                for line in body_lines {
                    raw_lines.push(Line::from(line));
                }
            }
            DetailTab::Headers => {
                raw_lines.push(Line::from(Span::styled(
                    "--- Request Headers ---",
                    Style::default().fg(t.info).add_modifier(Modifier::BOLD),
                )));
                if let Some(ref req) = state.current_request {
                    for (k, v) in &req.headers {
                        raw_lines.push(Line::from(format!("  {}: {}", k, v)));
                    }
                } else {
                    raw_lines.push(Line::from("  (No request headers)"));
                }
                raw_lines.push(Line::from(""));
                raw_lines.push(Line::from(Span::styled(
                    "--- Response Headers ---",
                    Style::default().fg(t.ok).add_modifier(Modifier::BOLD),
                )));
                let mut resp_keys: Vec<String> = resp
                    .headers
                    .keys()
                    .map(|k| k.as_str().to_string())
                    .collect();
                resp_keys.sort();
                for k in resp_keys {
                    if let Some(v) = resp.headers.get(&k) {
                        let val_str = v.to_str().unwrap_or("");
                        raw_lines.push(Line::from(format!("  {}: {}", k, val_str)));
                    }
                }
            }
            DetailTab::Request => {
                raw_lines.push(Line::from(Span::styled(
                    "--- Request Body ---",
                    Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                )));
                if let Some(ref req) = state.current_request {
                    if let Some(ref body) = req.body {
                        for line in body.lines() {
                            raw_lines.push(Line::from(line));
                        }
                    } else {
                        raw_lines.push(Line::from(Span::styled(
                            "  (No request body)",
                            Style::default().fg(t.dim),
                        )));
                    }
                } else {
                    raw_lines.push(Line::from(Span::styled(
                        "  (No request body)",
                        Style::default().fg(t.dim),
                    )));
                }
            }
        }

        // 调用 pre-wrapper 折行处理器 (保持 DRY)
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
