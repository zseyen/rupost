//! Response 臂与 HTTP 缓存管线：预折行重建 + 完成态切片渲染。

use super::headers::build_headers_lines;
use super::highlight::apply_search_highlight;
use super::request_tab::build_request_lines;
use crate::tui::state::AppState;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};

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

/// HTTP 缓存管线：按当前 DetailTab 构造原始行并预折行写入
/// `state.response_visual_lines`（渲染期唯一写点，由门面前置调用）。
pub(crate) fn rebuild_http_visual_lines(state: &mut AppState, max_width: usize) {
    let t = state.palette();
    use crate::tui::state::DetailTab;
    if let Some(ref resp) = state.last_response {
        let mut raw_lines = Vec::new();

        match state.active_detail_tab {
            // Results/Diagnose 有专属渲染臂，不经 HTTP 缓存管线
            DetailTab::Results | DetailTab::Diagnose => {}
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

                // R6：四段耗时（timing 存在时）
                if let Some(timing) = &state.last_timing {
                    raw_lines.push(Line::from(Span::styled(
                        format!(
                            "DNS {:?} · TCP {:?} · TTFB {:?} · Transfer {:?}",
                            timing.dns_lookup, timing.tcp_connect, timing.ttfb, timing.transfer
                        ),
                        Style::default().fg(t.dim),
                    )));
                }
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
                raw_lines.extend(build_headers_lines(state, resp));
            }
            DetailTab::Request => {
                raw_lines.extend(build_request_lines(state));
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

/// HTTP 完成态切片渲染（重建已由门面前置，此处只读）
pub(crate) fn render_http_completed(
    frame: &mut Frame,
    area: Rect,
    block: Block<'static>,
    state: &AppState,
    visible_height: usize,
) {
    let total_lines = state.response_visual_lines.len();
    let max_scroll = total_lines.saturating_sub(visible_height);
    let scroll_y = state.response_scroll.min(max_scroll);

    let end = (scroll_y + visible_height).min(total_lines);
    let sliced = state.response_visual_lines[scroll_y..end].to_vec();

    let sliced = apply_search_highlight(sliced, state);
    frame.render_widget(Paragraph::new(sliced).block(block), area);
}
