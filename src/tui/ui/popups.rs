use crate::tui::state::{AppState, InputMode};
use crate::tui::theme::Theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

/// 计算基于屏幕百分比居中的 Rect
pub(crate) fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

/// 计算基于绝对宽高（带屏幕防溢出约束）居中的 Rect
pub(crate) fn centered_rect_exact(width: u16, height: u16, screen: Rect) -> Rect {
    let w = width.min(screen.width.saturating_sub(4));
    let h = height.min(screen.height.saturating_sub(2));
    Rect::new(
        (screen.width.saturating_sub(w)) / 2,
        (screen.height.saturating_sub(h)) / 2,
        w,
        h,
    )
}

/// 渲染全局快捷键帮助面板
pub(crate) fn render_help(frame: &mut Frame, screen_size: Rect, t: &Theme) {
    let help_text = vec![
        Line::from(Span::styled(
            " RuPost TUI Shortcuts Reference ",
            Style::default().add_modifier(Modifier::BOLD).fg(t.accent),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Tab        ", Style::default().fg(t.info)),
            Span::raw(" Switch active panels (Files -> Editor -> Response)"),
        ]),
        Line::from(vec![
            Span::styled("  Left/Right ", Style::default().fg(t.info)),
            Span::raw(" Switch Sidebar sub-tabs (Files <-> History)"),
        ]),
        Line::from(vec![
            Span::styled("  p          ", Style::default().fg(t.info)),
            Span::raw(" Toggle display full file path in Files tab"),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+Enter ", Style::default().fg(t.info)),
            Span::raw(" Send current HTTP request"),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+R     ", Style::default().fg(t.info)),
            Span::raw(" Send current HTTP request (Backup key)"),
        ]),
        Line::from(vec![
            Span::styled("  Enter      ", Style::default().fg(t.info)),
            Span::raw(" In History tab, restore request to Preview"),
        ]),
        Line::from(vec![
            Span::styled("  /          ", Style::default().fg(t.info)),
            Span::raw(" Filter files (live, Esc clears)"),
        ]),
        Line::from(vec![
            Span::styled("  ,          ", Style::default().fg(t.accent)),
            Span::raw(" Toggle dark/light theme"),
        ]),
        Line::from(vec![
            Span::styled("  ?          ", Style::default().fg(t.info)),
            Span::raw(" Toggle this help panel"),
        ]),
        Line::from(vec![
            Span::styled("  q          ", Style::default().fg(t.info)),
            Span::raw(" Safe exit TUI mode"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  * History Storage: ~/.rupost/history.jsonl",
            Style::default().fg(t.dim),
        )),
        Line::from(Span::styled(
            "  * Run `rupost history show <Short ID>` in terminal to view details.",
            Style::default().fg(t.dim),
        )),
    ];

    let area = centered_rect_exact(74, 16, screen_size);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.accent));

    let paragraph = Paragraph::new(help_text).block(block);

    // 擦除底层界面，防止透字
    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

/// 渲染未保存修改强确认弹窗
pub(crate) fn render_unsaved_confirm(frame: &mut Frame, screen_size: Rect, t: &Theme) {
    let text = vec![
        Line::from(""),
        Line::from(Span::styled(
            " WARNING: Unsaved Changes! ",
            Style::default().add_modifier(Modifier::BOLD).fg(t.err),
        )),
        Line::from(""),
        Line::from(" You have unsaved modifications in the editor."),
        Line::from(" Do you want to discard them and continue?"),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  [y] ",
                Style::default().fg(t.ok).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Yes, discard changes"),
            Span::styled(
                "     [n/Esc] ",
                Style::default().fg(t.err).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" No, keep editing"),
        ]),
    ];

    let area = centered_rect_exact(50, 10, screen_size);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.err).add_modifier(Modifier::BOLD));

    let paragraph = Paragraph::new(text)
        .block(block)
        .alignment(ratatui::layout::Alignment::Center);

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

/// 渲染 X 键导出的 curl 命令行文本弹窗
pub(crate) fn render_export(frame: &mut Frame, screen_size: Rect, cmd: &str, t: &Theme) {
    let block = Block::default()
        .title(" Export: curl (Esc to close) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.accent));

    let area = centered_rect(80, 70, screen_size);
    let paragraph = Paragraph::new(Line::from(cmd))
        .block(block)
        .wrap(ratatui::widgets::Wrap { trim: false });

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

/// 渲染统一模态输入框（新建文件、正则过滤、单请求选择器 `#`、AI 断言 `I` 等）
pub(crate) fn render_modal_input(frame: &mut Frame, screen_size: Rect, state: &AppState) {
    let t = state.palette();
    let title = match &state.input_mode {
        InputMode::QuickCreate(_) => format!(" {} ", state.quick_input_title),
        InputMode::Filter(_) => " Filter files (substring, live): ".to_string(),
        InputMode::Auth(_) => {
            " Authorization header value (e.g. Bearer <token>, empty clears): ".to_string()
        }
        InputMode::Search(_) => " Search messages (n/N after Esc): ".to_string(),
        InputMode::WsSend(_) => " WS send (Enter to send): ".to_string(),
        InputMode::AiAssert(_) => {
            " AI assert (describe expectations, Enter to translate): ".to_string()
        }
        InputMode::Selector(_) => {
            format!(
                " Request selector (#3 or @name, empty clears){}: ",
                if state.run_selector.is_some() {
                    format!(" [active: {}]", state.run_selector.as_deref().unwrap_or(""))
                } else {
                    String::new()
                }
            )
        }
        InputMode::None => return,
    };
    let textarea = match &state.input_mode {
        InputMode::QuickCreate(t)
        | InputMode::Filter(t)
        | InputMode::Auth(t)
        | InputMode::Search(t)
        | InputMode::WsSend(t)
        | InputMode::Selector(t)
        | InputMode::AiAssert(t) => t,
        InputMode::None => return,
    };

    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(3), // 高度为 3
            Constraint::Min(0),
        ])
        .split(screen_size);

    let popup_area = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(60), // 宽度占 60%
            Constraint::Percentage(20),
        ])
        .split(popup_layout[1])[1];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.accent).add_modifier(Modifier::BOLD));

    frame.render_widget(Clear, popup_area);
    let block = block.title(title);
    frame.render_widget(block, popup_area);

    let inner = Rect {
        x: popup_area.x + 1,
        y: popup_area.y + 1,
        width: popup_area.width.saturating_sub(2),
        height: popup_area.height.saturating_sub(2),
    };
    frame.render_widget(textarea.as_ref(), inner);

    // 真实光标定位（screen_cursor 给出渲染后的绝对坐标）
    let cursor = textarea.screen_cursor();
    frame.set_cursor_position((inner.x + cursor.col as u16, inner.y + cursor.row as u16));
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn test_centered_rect_bounds() {
        let screen = Rect::new(0, 0, 100, 50);
        let area = centered_rect(50, 50, screen);
        assert_eq!(area.width, 50);
        assert_eq!(area.height, 25);
        assert_eq!(area.x, 25);
        assert_eq!(area.y, 13);

        let exact = centered_rect_exact(60, 20, screen);
        assert_eq!(exact.width, 60);
        assert_eq!(exact.height, 20);
        assert_eq!(exact.x, 20);
        assert_eq!(exact.y, 15);
    }

    #[test]
    fn test_offscreen_render_popups() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = crate::tui::theme::palette(crate::tui::theme::ThemeKind::Dark);

        terminal
            .draw(|f| {
                render_help(f, f.area(), theme);
            })
            .unwrap();

        terminal
            .draw(|f| {
                render_unsaved_confirm(f, f.area(), theme);
            })
            .unwrap();

        terminal
            .draw(|f| {
                render_export(f, f.area(), "curl -X GET http://example.com", theme);
            })
            .unwrap();
    }
}

