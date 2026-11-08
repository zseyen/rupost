//! Request 臂内容构造（经 HTTP 缓存管线，rebuild 分发调用）。

use crate::tui::state::AppState;
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

/// Request Tab 的原始行构造（请求 Body）
pub(crate) fn build_request_lines(state: &AppState) -> Vec<Line<'static>> {
    let t = state.palette();
    let mut raw_lines = Vec::new();
    raw_lines.push(Line::from(Span::styled(
        "--- Request Body ---",
        Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
    )));
    if let Some(ref req) = state.current_request {
        if let Some(ref body) = req.body {
            for line in body.lines() {
                raw_lines.push(Line::from(line.to_string()));
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
    raw_lines
}
