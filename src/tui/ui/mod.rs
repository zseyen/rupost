use super::state::{AppState, LayoutMode, Panel};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Tabs, Wrap},
};

/// 渲染 TUI 面板布局主入口
pub fn render(
    frame: &mut Frame,
    state: &mut AppState,
    textarea: &mut ratatui_textarea::TextArea<'static>,
) {
    let size = frame.area();

    // 1. 终端超小防御机制
    if size.width < 40 || size.height < 10 {
        let warning = Paragraph::new("Terminal too small. Please enlarge the window.")
            .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
            .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(warning, size);
        return;
    }

    // 2. 根据自适应模式划分主显示区
    match state.layout_mode {
        LayoutMode::Wide => {
            // 三栏并排
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(25), // 📂 Files
                    Constraint::Percentage(40), // 📝 Request Editor
                    Constraint::Percentage(35), // 📊 Response Viewer
                ])
                .split(size);

            render_files_panel(frame, chunks[0], state);
            render_editor_panel(frame, chunks[1], state, textarea);
            render_response_panel(frame, chunks[2], state);
        }
        LayoutMode::Narrow => {
            // 双栏并排
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(50), // 📝 Request Editor
                    Constraint::Percentage(50), // 📊 Response Viewer
                ])
                .split(size);

            render_editor_panel(frame, chunks[0], state, textarea);
            render_response_panel(frame, chunks[1], state);
        }
        LayoutMode::Stacked => {
            // 单栏堆叠 (通过 Tab 切换显示)
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3), // 顶部 Tab 栏
                    Constraint::Min(0),    // 主要显示区域
                ])
                .split(size);

            // 渲染 Tab 导航头
            let titles = vec!["[1] Files", "[2] Editor", "[3] Response"];
            let active_idx = match state.active_panel {
                Panel::Files => 0,
                Panel::Editor => 1,
                Panel::Response => 2,
            };

            let tabs = Tabs::new(titles)
                .block(Block::default().borders(Borders::BOTTOM))
                .select(active_idx)
                .style(Style::default().fg(Color::DarkGray))
                .highlight_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                );

            frame.render_widget(tabs, chunks[0]);

            // 根据当前激活面板进行渲染
            match state.active_panel {
                Panel::Files => render_files_panel(frame, chunks[1], state),
                Panel::Editor => render_editor_panel(frame, chunks[1], state, textarea),
                Panel::Response => render_response_panel(frame, chunks[1], state),
            }
        }
    }

    // 3. 渲染全局帮助悬浮窗 (Modal Panel)
    if state.show_help {
        render_help_popup(frame, size);
    }

    // 4. 渲染未保存强确认弹窗 (Unsaved Changes Alert Modal)
    if state.show_unsaved_confirm {
        render_unsaved_popup(frame, size);
    }
}

fn render_files_panel(frame: &mut Frame, area: Rect, state: &AppState) {
    let focus = state.active_panel == Panel::Files;
    let border_color = if focus { Color::Cyan } else { Color::DarkGray };

    let block = Block::default()
        .title(" Files & History ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let content = if state.file_tree.is_empty() {
        Paragraph::new("No files found in workspace.").style(Style::default().fg(Color::DarkGray))
    } else {
        let lines: Vec<Line> = state
            .file_tree
            .iter()
            .enumerate()
            .map(|(idx, f)| {
                let style = if idx == state.selected_file_index {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                Line::from(Span::styled(format!("  {}", f), style))
            })
            .collect();
        Paragraph::new(lines)
    };

    frame.render_widget(content.block(block), area);
}

fn render_editor_panel(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    textarea: &mut ratatui_textarea::TextArea<'static>,
) {
    let focus = state.active_panel == Panel::Editor;
    let border_color = if focus { Color::Cyan } else { Color::DarkGray };

    let title = if state.is_dirty {
        " Request Editor * "
    } else {
        " Request Editor "
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    textarea.set_block(block);

    frame.render_widget(&*textarea, area);
}

fn render_response_panel(frame: &mut Frame, area: Rect, state: &AppState) {
    let focus = state.active_panel == Panel::Response;
    let border_color = if focus { Color::Cyan } else { Color::DarkGray };

    let block = Block::default()
        .title(" Response Viewer ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let text = if state.is_loading {
        Paragraph::new("Executing request, please wait...")
            .style(Style::default().fg(Color::Yellow))
    } else if let Some(ref resp) = state.last_response {
        let status_color = if resp.is_success() {
            Color::Green
        } else {
            Color::Red
        };

        let mut lines = vec![
            Line::from(vec![
                Span::styled("Status: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{} {}", resp.status.code(), resp.status.reason_phrase()),
                    Style::default()
                        .fg(status_color)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("Time: ", Style::default().fg(Color::DarkGray)),
                Span::raw(format!("{}ms  ", resp.duration.as_millis())),
                Span::styled("Size: ", Style::default().fg(Color::DarkGray)),
                Span::raw(format!("{} bytes", resp.body.len())),
            ]),
            Line::from(""),
            Line::from(Span::styled("Body:", Style::default().fg(Color::Yellow))),
        ];

        for line in resp.body.lines() {
            lines.push(Line::from(line));
        }

        let total_lines = lines.len();
        let visible_height = area.height.saturating_sub(2) as usize;
        let max_scroll = total_lines
            .saturating_sub(visible_height)
            .min(u16::MAX as usize) as u16;
        let scroll_y = state.response_scroll.min(max_scroll);

        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .scroll((scroll_y, 0))
    } else {
        Paragraph::new("No response data. Trigger execution via Ctrl+Enter.")
            .style(Style::default().fg(Color::DarkGray))
    };

    frame.render_widget(text.block(block), area);
}

fn render_help_popup(frame: &mut Frame, screen_size: Rect) {
    let help_text = vec![
        Line::from(Span::styled(
            " RuPost TUI Shortcuts Reference ",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Yellow),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Tab        ", Style::default().fg(Color::Cyan)),
            Span::raw(" Switch active panels"),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+Enter ", Style::default().fg(Color::Cyan)),
            Span::raw(" Send current HTTP request"),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+R     ", Style::default().fg(Color::Cyan)),
            Span::raw(" Send current HTTP request (Backup key)"),
        ]),
        Line::from(vec![
            Span::styled("  Ctrl+N     ", Style::default().fg(Color::Cyan)),
            Span::raw(" Open Quick Raw Request bar"),
        ]),
        Line::from(vec![
            Span::styled("  ?          ", Style::default().fg(Color::Cyan)),
            Span::raw(" Toggle this help panel"),
        ]),
        Line::from(vec![
            Span::styled("  q          ", Style::default().fg(Color::Cyan)),
            Span::raw(" Safe exit TUI mode"),
        ]),
    ];

    let width = 50.min(screen_size.width - 4);
    let height = 12.min(screen_size.height - 2);

    let area = Rect::new(
        (screen_size.width - width) / 2,
        (screen_size.height - height) / 2,
        width,
        height,
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let paragraph = Paragraph::new(help_text).block(block);

    // 擦除底层界面，防止透字
    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}

fn render_unsaved_popup(frame: &mut Frame, screen_size: Rect) {
    let text = vec![
        Line::from(""),
        Line::from(Span::styled(
            " WARNING: Unsaved Changes! ",
            Style::default().add_modifier(Modifier::BOLD).fg(Color::Red),
        )),
        Line::from(""),
        Line::from(" You have unsaved modifications in the editor."),
        Line::from(" Do you want to discard them and continue?"),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  [y] ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Yes, discard changes"),
            Span::styled(
                "     [n/Esc] ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" No, keep editing"),
        ]),
    ];

    let width = 50.min(screen_size.width - 4);
    let height = 10.min(screen_size.height - 2);

    let area = Rect::new(
        (screen_size.width - width) / 2,
        (screen_size.height - height) / 2,
        width,
        height,
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD));

    let paragraph = Paragraph::new(text)
        .block(block)
        .alignment(ratatui::layout::Alignment::Center);

    frame.render_widget(Clear, area);
    frame.render_widget(paragraph, area);
}
