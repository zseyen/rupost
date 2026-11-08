//! Results 臂（R3）：多请求执行结果摘要渲染。

use crate::tui::state::AppState;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};

pub(crate) fn render(frame: &mut Frame, area: Rect, block: Block<'static>, state: &AppState) {
    let t = state.palette();
    let mut lines: Vec<Line> = Vec::new();
    if state.run_total == 0 {
        lines.push(Line::from(Span::styled(
            "No run in this session. Trigger execution to collect results.",
            Style::default().fg(t.dim),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            format!(
                "Run progress: {}/{} (selector: {})",
                state.run_results.len(),
                state.run_total,
                state.run_selector.as_deref().unwrap_or("-")
            ),
            Style::default().fg(t.info),
        )));
        lines.push(Line::from(""));
        for r in &state.run_results {
            let name = r
                .name
                .clone()
                .unwrap_or_else(|| format!("request #{}", r.request_number));
            let (status_text, color) = if r.skipped {
                ("SKIP".to_string(), t.dim)
            } else if r.success {
                (
                    format!(
                        "PASS {}",
                        r.status.map(|s| s.to_string()).unwrap_or_default()
                    ),
                    t.ok,
                )
            } else {
                (
                    format!("FAIL {}", r.error.clone().unwrap_or_default()),
                    t.err,
                )
            };
            let passed = r.assertions.iter().filter(|a| a.passed).count();
            lines.push(Line::from(vec![
                Span::styled(
                    format!("#{:<3} ", r.request_number),
                    Style::default().fg(t.dim),
                ),
                Span::styled(format!("{name} "), Style::default().fg(t.text)),
                Span::styled(
                    format!("{status_text} "),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{}ms ", r.duration.as_millis()),
                    Style::default().fg(t.dim),
                ),
                Span::styled(
                    format!("assertions {}/{} passed", passed, r.assertions.len()),
                    Style::default().fg(if passed == r.assertions.len() {
                        t.ok
                    } else {
                        t.err
                    }),
                ),
            ]));
            lines.push(Line::from(Span::styled(
                format!("     {}", r.url),
                Style::default().fg(t.dim),
            )));
        }
        if state.run_results.len() < state.run_total {
            lines.push(Line::from(Span::styled(
                format!("… running {}/{}", state.run_results.len(), state.run_total),
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            )));
        }
    }
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}
