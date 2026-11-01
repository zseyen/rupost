use crate::tui::state::{AppState, DetailTab, Panel};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs},
};

/// 对可见切片应用搜索高亮（query 为空时原样返回）
fn apply_search_highlight(sliced: Vec<Line<'static>>, state: &AppState) -> Vec<Line<'static>> {
    let query = state.search_query.clone();
    if query.is_empty() {
        return sliced;
    }
    let accent = state.palette().accent;
    sliced
        .into_iter()
        .map(|l| highlight_query_line(&l.to_string(), &query, accent))
        .collect()
}

/// 流式（SSE/WS）完成态视图首行插入告警摘要（D1：流式分支不走 HTTP 缓存管线）。
/// 仅 scroll=0 时可见（作为视图头行）；滚动后随内容移出视口。
fn prepend_warnings_summary(lines: &mut Vec<Line<'static>>, state: &AppState) {
    if state.warnings.is_empty() {
        return;
    }
    let t = state.palette();
    lines.insert(
        0,
        Line::from(Span::styled(
            format!("Warnings: {}", state.warnings.len()),
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        )),
    );
}

/// 在原串字符边界上做大小写不敏感子串匹配，返回命中字节区间（升序、不重叠）。
/// 不采用 `to_lowercase()` 整体偏移：它不保字节长度（如 `İ` → `i`+`◌̇`、`ß` → `ss`），
/// 拿小写串的偏移切原串会落入多字节字符内部导致 panic（char boundary 越界）。
fn find_match_ranges(line: &str, query: &str) -> Vec<(usize, usize)> {
    if query.is_empty() || line.is_empty() {
        return Vec::new();
    }
    let offsets: Vec<usize> = line.char_indices().map(|(o, _)| o).collect();
    // 每个原字符的小写展开（可能 1→多字符），记录其源字符下标
    let lowered: Vec<(usize, char)> = line
        .char_indices()
        .enumerate()
        .flat_map(|(i, (_, ch))| ch.to_lowercase().map(move |c| (i, c)))
        .collect();
    let ql: Vec<char> = query.chars().flat_map(|c| c.to_lowercase()).collect();
    if ql.is_empty() {
        return Vec::new();
    }
    let mut ranges = Vec::new();
    let mut i = 0;
    while i + ql.len() <= lowered.len() {
        if lowered[i..i + ql.len()]
            .iter()
            .map(|&(_, c)| c)
            .eq(ql.iter().copied())
        {
            let start = offsets[lowered[i].0];
            let end = offsets
                .get(lowered[i + ql.len() - 1].0 + 1)
                .copied()
                .unwrap_or(line.len());
            ranges.push((start, end));
            i += ql.len();
        } else {
            i += 1;
        }
    }
    ranges
}

/// 搜索高亮（R4）：把行文本按 query 大小写不敏感切分，命中段 accent 背景 + 加粗。
/// query 为空时原样返回；仅作用于当前可见切片（不影响底层视觉行缓存）。
fn highlight_query_line(line: &str, query: &str, accent: ratatui::style::Color) -> Line<'static> {
    use ratatui::style::{Color, Style};
    if query.is_empty() {
        return Line::from(line.to_string());
    }
    let mut spans = Vec::new();
    let mut cursor = 0;
    for (start, end) in find_match_ranges(line, query) {
        if start > cursor {
            spans.push(Span::raw(line[cursor..start].to_string()));
        }
        spans.push(Span::styled(
            line[start..end].to_string(),
            Style::default().bg(accent).fg(Color::Black),
        ));
        cursor = end;
    }
    if cursor < line.len() {
        spans.push(Span::raw(line[cursor..].to_string()));
    }
    Line::from(spans)
}

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
    } else if state.ws_session.is_some() {
        " Response Viewer [WS Interactive] ".to_string()
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
    // 交互会话（R6）无 runner 日志文件参与，帧只存在于内存视觉行——
    // 必须与流式态同路渲染，否则视口恒空且可能串入上一轮运行的旧日志
    let session_active = state.ws_session.is_some();

    if use_streaming && is_ws {
        let visual_lines = if state.is_loading || session_active {
            state.ws_visual_lines.clone()
        } else if state.log_file_path.is_some() {
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
        } else {
            // 完成态但无日志文件（历史运行/无日志场景）：回退内存帧，与搜索空间一致
            state.ws_visual_lines.clone()
        };

        let mut visual_lines_final = visual_lines;
        if visual_lines_final.is_empty() {
            visual_lines_final.push(Line::from(Span::styled(
                "WebSocket connected. Listening for frames...",
                Style::default().fg(t.dim),
            )));
        }
        prepend_warnings_summary(&mut visual_lines_final, state);

        // 3. 切片：流式态/交互会话 = 底部钳制；完成态 = scroll 即视口顶部日志行号
        //    （滑窗前后各 50 行前导缓冲，视口起点 = min(scroll, 50) + warnings 偏移；
        //    此前按窗口局部长度钳制，日志行号 >50 的搜索跳转目标会落在视口外）
        let sliced = if state.is_loading || session_active {
            let total_lines = visual_lines_final.len();
            let max_scroll = total_lines.saturating_sub(visible_height);
            let scroll_y = state.response_scroll.min(max_scroll);
            let end = (scroll_y + visible_height).min(total_lines);
            visual_lines_final[scroll_y..end].to_vec()
        } else {
            let warn = usize::from(!state.warnings.is_empty());
            let lead = if state.response_scroll == 0 {
                0
            } else {
                warn + state.response_scroll.min(50)
            }
            .min(visual_lines_final.len());
            let end = (lead + visible_height).min(visual_lines_final.len());
            visual_lines_final[lead..end].to_vec()
        };

        let sliced = apply_search_highlight(sliced, state);
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
        prepend_warnings_summary(&mut visual_lines_final, state);

        // 切片语义与 WS 完成态相同（scroll = 视口顶部日志行号）
        let sliced = if state.is_loading {
            let total_lines = visual_lines_final.len();
            let max_scroll = total_lines.saturating_sub(visible_height);
            let scroll_y = state.response_scroll.min(max_scroll);
            let end = (scroll_y + visible_height).min(total_lines);
            visual_lines_final[scroll_y..end].to_vec()
        } else {
            let warn = usize::from(!state.warnings.is_empty());
            let lead = if state.response_scroll == 0 {
                0
            } else {
                warn + state.response_scroll.min(50)
            }
            .min(visual_lines_final.len());
            let end = (lead + visible_height).min(visual_lines_final.len());
            visual_lines_final[lead..end].to_vec()
        };

        let sliced = apply_search_highlight(sliced, state);
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

        let sliced = apply_search_highlight(sliced, state);
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

/// 将借用行转为 'static 行并保留各 Span 样式（response_visual_lines 缓存要求 'static）
fn into_static_line(line: Line<'_>) -> Line<'static> {
    let mut owned = Line::from(
        line.spans
            .into_iter()
            .map(|s| Span::styled(s.content.into_owned(), s.style))
            .collect::<Vec<_>>(),
    );
    owned.style = line.style;
    owned.alignment = line.alignment;
    owned
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
                        Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
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
            // 未超宽的行原样保留 Span 样式（主题取色/加粗）；into_static_line 断开对
            // state.last_response 的借用（缓存要求 'static）。
            // 仅真正发生折行的行才扁平化为纯文本（wrap_text 无法跨 Span 切分）
            if unicode_width::UnicodeWidthStr::width(line_str.as_str()) <= max_width {
                wrapped.push(into_static_line(line));
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

#[cfg(test)]
mod highlight_tests {
    use super::*;

    const ACCENT: ratatui::style::Color = ratatui::style::Color::Rgb(1, 2, 3);

    fn spans_of(line: Line<'static>) -> Vec<(String, bool)> {
        line.spans
            .iter()
            .map(|s| {
                let styled = s.style.bg == Some(ACCENT);
                (s.content.to_string(), styled)
            })
            .collect()
    }

    /// 高亮必须保持全文拼接不变（任何切分错误首先破坏这一点）
    fn assert_roundtrip(line: &str, query: &str) {
        let l = highlight_query_line(line, query, ACCENT);
        let joined: String = l.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(joined, line, "query={query:?} must preserve full text");
    }

    #[test]
    fn test_highlight_ascii_basic_segments() {
        let l = highlight_query_line("data: hello world", "hello", ACCENT);
        let spans = spans_of(l);
        assert_eq!(
            spans,
            vec![
                ("data: ".to_string(), false),
                ("hello".to_string(), true),
                (" world".to_string(), false),
            ]
        );
        assert_roundtrip("data: hello world", "hello");
    }

    /// P0-1 回归锚点：`İ`（U+0130，2 字节）小写化为 `i`+U+0307（3 字节），
    /// 旧实现拿小写串字节偏移切原串 → char boundary panic（UI 崩溃）
    #[test]
    fn test_highlight_turkish_capital_i_no_panic() {
        // 旧实现此输入确定性 panic；命中段必须落在原字符边界上
        let l = highlight_query_line("İx", "i", ACCENT);
        let spans = spans_of(l);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0], ("İ".to_string(), true), "İ must be the hit span");
        assert_eq!(spans[1], ("x".to_string(), false));
        assert_roundtrip("İx", "i");
    }

    /// `ß` 恒等小写（展开 "ss" 的是 to_uppercase）：多字节字符不干扰后续命中偏移
    #[test]
    fn test_highlight_sharp_s_boundary() {
        let l = highlight_query_line("ßSS", "ss", ACCENT);
        let spans = spans_of(l);
        assert_eq!(
            spans,
            vec![("ß".to_string(), false), ("SS".to_string(), true)]
        );
        assert_roundtrip("ßSS", "ss");
    }

    /// CJK 多字节字符（每字符 3 字节）边界：命中段必须整字符覆盖
    #[test]
    fn test_highlight_cjk_boundary() {
        let l = highlight_query_line("你好世界", "好世", ACCENT);
        let spans = spans_of(l);
        assert_eq!(
            spans,
            vec![
                ("你".to_string(), false),
                ("好世".to_string(), true),
                ("界".to_string(), false),
            ]
        );
        assert_roundtrip("你好世界", "好");
    }

    #[test]
    fn test_highlight_no_match_single_raw_span() {
        let l = highlight_query_line("plain text", "zzz", ACCENT);
        assert_eq!(spans_of(l), vec![("plain text".to_string(), false)]);
        // 空 query 原样返回
        let l = highlight_query_line("plain text", "", ACCENT);
        assert_eq!(spans_of(l), vec![("plain text".to_string(), false)]);
    }

    /// 多命中不重叠推进：连续命中间的非命中段保留
    #[test]
    fn test_highlight_multiple_hits_with_gaps() {
        let l = highlight_query_line("ab ab ab", "ab", ACCENT);
        let spans = spans_of(l);
        assert_eq!(
            spans,
            vec![
                ("ab".to_string(), true),
                (" ".to_string(), false),
                ("ab".to_string(), true),
                (" ".to_string(), false),
                ("ab".to_string(), true),
            ]
        );
    }
}
