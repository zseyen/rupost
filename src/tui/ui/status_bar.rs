use crate::tui::state::{AppState, Panel, SidebarTab};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

/// 渲染底部 lazygit 风格状态栏与上下文快捷键指示
pub(crate) fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    let t = state.palette();
    let style = Style::default().bg(t.status_bg).fg(t.status_fg);

    // 写路径失败优先呈现（替代快捷键提示；下一次成功操作清除）
    if let Some(err) = &state.last_error {
        let spans = vec![
            Span::styled(
                " ! ",
                Style::default().fg(t.err).add_modifier(Modifier::BOLD),
            ),
            Span::styled(err.clone(), Style::default().fg(t.err)),
        ];
        frame.render_widget(Paragraph::new(Line::from(spans)).style(style), area);
        return;
    }

    // 快速授权指示（R3）与 mock 会话指示（R11）：全面板可见
    let mut spans = Vec::new();
    if state.quick_auth().is_some() {
        spans.push(Span::styled(
            " AUTH✓ ",
            Style::default().fg(t.ok).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw("│ "));
    }
    if let Some(sess) = &state.mock_session {
        spans.push(Span::styled(
            format!(" MOCK :{} ", sess.port),
            Style::default().fg(t.ok).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw("│ "));
    }

    // 根据当前活跃面板及侧边栏 Tab 呈现上下文敏捷按键映射
    spans.extend(match state.active_panel {
        Panel::Files => match state.active_sidebar_tab {
            SidebarTab::Files => vec![
                Span::styled(
                    " q ",
                    Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                ),
                Span::raw("Quit │ "),
                Span::styled(
                    " ? ",
                    Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                ),
                Span::raw("Help │ "),
                Span::styled(
                    " Tab ",
                    Style::default().fg(t.info).add_modifier(Modifier::BOLD),
                ),
                Span::raw("Focus Editor │ "),
                Span::styled(
                    " h/l (←/→) ",
                    Style::default().fg(t.info).add_modifier(Modifier::BOLD),
                ),
                Span::raw("Switch Tab │ "),
                Span::styled(
                    " j/k (↑/↓) / Click ",
                    Style::default().fg(t.info).add_modifier(Modifier::BOLD),
                ),
                Span::raw("Select │ "),
                Span::styled(
                    " Enter ",
                    Style::default().fg(t.ok).add_modifier(Modifier::BOLD),
                ),
                Span::raw("Edit File │ "),
                Span::styled(
                    " p ",
                    Style::default().fg(t.info).add_modifier(Modifier::BOLD),
                ),
                Span::raw("Toggle Path"),
            ],
            SidebarTab::History => vec![
                Span::styled(
                    " q ",
                    Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                ),
                Span::raw("Quit │ "),
                Span::styled(
                    " ? ",
                    Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                ),
                Span::raw("Help │ "),
                Span::styled(
                    " Tab ",
                    Style::default().fg(t.info).add_modifier(Modifier::BOLD),
                ),
                Span::raw("Focus Editor │ "),
                Span::styled(
                    " h/l (←/→) ",
                    Style::default().fg(t.info).add_modifier(Modifier::BOLD),
                ),
                Span::raw("Switch Tab │ "),
                Span::styled(
                    " j/k (↑/↓) / Click ",
                    Style::default().fg(t.info).add_modifier(Modifier::BOLD),
                ),
                Span::raw("Select │ "),
                Span::styled(
                    " Enter ",
                    Style::default().fg(t.ok).add_modifier(Modifier::BOLD),
                ),
                Span::raw("Load History"),
            ],
        },
        Panel::Editor => vec![
            Span::styled(
                " q ",
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            ),
            Span::raw("Quit │ "),
            Span::styled(
                " ? ",
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            ),
            Span::raw("Help │ "),
            Span::styled(
                " Tab ",
                Style::default().fg(t.info).add_modifier(Modifier::BOLD),
            ),
            Span::raw("Focus Response │ "),
            Span::styled(
                " Ctrl+Enter / Ctrl+R ",
                Style::default().fg(t.ok).add_modifier(Modifier::BOLD),
            ),
            Span::raw("Run Request │ "),
            Span::styled(
                " Ctrl+S ",
                Style::default().fg(t.info).add_modifier(Modifier::BOLD),
            ),
            Span::raw("Save"),
        ],
        Panel::Response => vec![
            Span::styled(
                " q ",
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            ),
            Span::raw("Quit │ "),
            Span::styled(
                " ? ",
                Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
            ),
            Span::raw("Help │ "),
            Span::styled(
                " Tab ",
                Style::default().fg(t.info).add_modifier(Modifier::BOLD),
            ),
            Span::raw("Focus Sidebar │ "),
            Span::styled(
                " j/k (↑/↓) ",
                Style::default().fg(t.info).add_modifier(Modifier::BOLD),
            ),
            Span::raw("Scroll Response"),
        ],
    });

    let paragraph = Paragraph::new(Line::from(spans)).style(style);
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn test_status_bar_error_renders_priority() {
        let backend = TestBackend::new(80, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = AppState::new();
        state.last_error = Some("network connection timeout".to_string());

        terminal
            .draw(|f| {
                render(f, f.area(), &state);
            })
            .unwrap();
    }

    #[tokio::test]
    async fn test_status_bar_quick_auth_and_mock_session() {
        let backend = TestBackend::new(80, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = AppState::new();
        state.mock_session = Some(crate::tui::integrations::MockSessionState {
            file: "test.http".to_string(),
            port: 8080,
            abort: tokio::spawn(async {}).abort_handle(),
        });

        terminal
            .draw(|f| {
                render(f, f.area(), &state);
            })
            .unwrap();
    }
}
