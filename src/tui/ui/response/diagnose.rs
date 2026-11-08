//! Diagnose 臂（R7）：失败自动诊断数据渲染（TUI 原生渲染，非 ANSI 直印）。

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
    match &state.last_diagnose {
        None => {
            lines.push(Line::from(Span::styled(
                "No diagnose data for the last request.",
                Style::default().fg(t.dim),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Diagnose runs automatically when a request fails (debug-on-failure semantics).",
                Style::default().fg(t.dim),
            )));
        }
        Some(d) => {
            let kv = |k: &str, v: String| {
                Line::from(vec![
                    Span::styled(format!("{:<22}", k), Style::default().fg(t.dim)),
                    Span::raw(v),
                ])
            };
            lines.push(Line::from(Span::styled(
                "Network Diagnostics (auto on failure)",
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("URL: ", Style::default().fg(t.dim)),
                Span::raw(d.url.clone()),
            ]));
            lines.push(kv(
                "Scheme:",
                if d.is_https { "https" } else { "http" }.to_string(),
            ));
            if let Some(status) = d.http_status {
                lines.push(kv("HTTP Status:", status.to_string()));
            }
            if let Some(ref ver) = d.http_version {
                lines.push(kv("HTTP Version:", ver.clone()));
            }
            lines.push(kv(
                "Resolved IPs:",
                if d.resolved_ips.is_empty() {
                    "-".to_string()
                } else {
                    d.resolved_ips.join(", ")
                },
            ));
            lines.push(kv("DNS Lookup:", format!("{:?}", d.dns_lookup_duration)));
            lines.push(kv("TCP Connect:", format!("{:?}", d.tcp_connect_duration)));
            if let Some(tls) = d.tls_handshake_duration {
                lines.push(kv("TLS Handshake:", format!("{tls:?}")));
            }
            if let Some(ttfb) = d.ttfb {
                lines.push(kv("TTFB:", format!("{ttfb:?}")));
            }
            lines.push(kv("Total:", format!("{:?}", d.total_duration)));
            if let Some(ref cert) = d.cert_info {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "Certificate:",
                    Style::default().fg(t.accent),
                )));
                lines.push(kv("  Subject:", cert.subject.clone()));
                lines.push(kv("  Expires in:", format!("{} days", cert.days_remaining)));
                if !cert.sans.is_empty() {
                    lines.push(kv("  SANs:", cert.sans.join(", ")));
                }
            }
        }
    }
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}
