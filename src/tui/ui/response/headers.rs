//! Headers 臂内容构造（经 HTTP 缓存管线，rebuild 分发调用）。

use crate::http::Response;
use crate::tui::state::AppState;
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

/// Headers Tab 的原始行构造（请求头 + 响应头）
pub(crate) fn build_headers_lines(state: &AppState, resp: &Response) -> Vec<Line<'static>> {
    let t = state.palette();
    let mut raw_lines = Vec::new();
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
            // lossy 读回：非 ASCII 值不再静默空串（与非 ASCII header 修复批次同口径）
            let val_str = String::from_utf8_lossy(v.as_bytes());
            raw_lines.push(Line::from(format!("  {}: {}", k, val_str)));
        }
    }
    raw_lines
}
