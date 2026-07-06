pub mod sidebar;
pub mod editor;
pub mod response;

use super::state::{AppState, LayoutMode, Panel};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Tabs},
};

/// 渲染 TUI 面板布局主入口
pub fn render(
    frame: &mut Frame,
    state: &mut AppState,
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

    // 划分出最底下一行的 Help Bar 区域
    let screen_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(size);
    let main_area = screen_chunks[0];
    let help_bar_area = screen_chunks[1];

    // 2. 根据自适应模式划分主显示区，分派子组件渲染
    match state.layout_mode {
        LayoutMode::Wide => {
            // 三栏并排
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(25), // 📂 Files & History (Sidebar)
                    Constraint::Percentage(40), // 📝 Request Editor
                    Constraint::Percentage(35), // 📊 Response Viewer
                ])
                .split(main_area);

            sidebar::render(frame, chunks[0], state);
            editor::render(frame, chunks[1], state);
            response::render(frame, chunks[2], state);
        }
        LayoutMode::Narrow => {
            // 重构为三栏并排 (20% sidebar, 40% editor, 40% response)
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(20), // 📂 Files & History (Sidebar)
                    Constraint::Percentage(40), // 📝 Request Editor
                    Constraint::Percentage(40), // 📊 Response Viewer
                ])
                .split(main_area);

            sidebar::render(frame, chunks[0], state);
            editor::render(frame, chunks[1], state);
            response::render(frame, chunks[2], state);
        }
        LayoutMode::Stacked => {
            // 单栏堆叠 (通过 Tab 切换显示)
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3), // 顶部 Tab 栏
                    Constraint::Min(0),    // 主要显示区域
                ])
                .split(main_area);

            // 渲染大 Tab 导航头
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
                Panel::Files => sidebar::render(frame, chunks[1], state),
                Panel::Editor => editor::render(frame, chunks[1], state),
                Panel::Response => response::render(frame, chunks[1], state),
            }
        }
    }

    // 3. 渲染全局帮助悬浮窗 (Modal Panel)
    if state.show_help {
        render_help_popup(frame, main_area);
    }

    // 4. 渲染未保存强确认弹窗 (Unsaved Changes Alert Modal)
    if state.show_unsaved_confirm {
        render_unsaved_popup(frame, main_area);
    }

    // 5. 渲染底部 lazygit 风格状态栏
    render_help_bar(frame, help_bar_area, state);
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
            Span::raw(" Switch active panels (Files -> Editor -> Response)"),
        ]),
        Line::from(vec![
            Span::styled("  Left/Right ", Style::default().fg(Color::Cyan)),
            Span::raw(" Switch Sidebar sub-tabs (Files <-> History)"),
        ]),
        Line::from(vec![
            Span::styled("  p          ", Style::default().fg(Color::Cyan)),
            Span::raw(" Toggle display full file path in Files tab"),
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
            Span::styled("  ?          ", Style::default().fg(Color::Cyan)),
            Span::raw(" Toggle this help panel"),
        ]),
        Line::from(vec![
            Span::styled("  q          ", Style::default().fg(Color::Cyan)),
            Span::raw(" Safe exit TUI mode"),
        ]),
    ];

    let width = 60.min(screen_size.width - 4);
    let height = 13.min(screen_size.height - 2);

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

fn render_help_bar(frame: &mut Frame, area: Rect, state: &AppState) {
    use crate::tui::state::{Panel, SidebarTab};
    
    let style = Style::default()
        .bg(Color::Rgb(30, 30, 46))
        .fg(Color::Rgb(205, 214, 244));
        
    let spans = match state.active_panel {
        Panel::Files => match state.active_sidebar_tab {
            SidebarTab::Files => vec![
                Span::styled(" q ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw("Quit │ "),
                Span::styled(" ? ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw("Help │ "),
                Span::styled(" Tab ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Focus Editor │ "),
                Span::styled(" h/l (←/→) ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Switch Tab │ "),
                Span::styled(" j/k (↑/↓) / Click ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Select │ "),
                Span::styled(" Enter ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw("Edit File │ "),
                Span::styled(" p ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Toggle Path"),
            ],
            SidebarTab::History => vec![
                Span::styled(" q ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw("Quit │ "),
                Span::styled(" ? ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw("Help │ "),
                Span::styled(" Tab ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Focus Editor │ "),
                Span::styled(" h/l (←/→) ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Switch Tab │ "),
                Span::styled(" j/k (↑/↓) / Click ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw("Select │ "),
                Span::styled(" Enter ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw("Load History"),
            ],
        },
        Panel::Editor => vec![
            Span::styled(" q ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw("Quit │ "),
            Span::styled(" ? ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw("Help │ "),
            Span::styled(" Tab ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("Focus Response │ "),
            Span::styled(" Ctrl+Enter / Ctrl+R ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw("Run Request │ "),
            Span::styled(" Ctrl+S ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("Save"),
        ],
        Panel::Response => vec![
            Span::styled(" q ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw("Quit │ "),
            Span::styled(" ? ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw("Help │ "),
            Span::styled(" Tab ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("Focus Sidebar │ "),
            Span::styled(" j/k (↑/↓) ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("Scroll Response"),
        ],
    };

    let paragraph = Paragraph::new(Line::from(spans)).style(style);
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::state::{AppState, LayoutMode, Panel, SidebarTab};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn smoke_test_components_render() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::new();
        state.file_tree = vec![
            "test1.http".to_string(),
            "subdir/test2.http".to_string(),
        ];
        state.history_list = vec![
            crate::history::model::HistoryEntry {
                id: "1".to_string(),
                timestamp: chrono::Utc::now(),
                duration_ms: 15,
                request: crate::history::model::RequestSnapshot {
                    method: "GET".to_string(),
                    url: "http://example.com/api".to_string(),
                    headers: reqwest::header::HeaderMap::new(),
                    body: None,
                },
                source: None,
                response: crate::history::model::ResponseMeta {
                    status: 200,
                    headers: reqwest::header::HeaderMap::new(),
                    body: Some("{}".to_string()),
                },
            }
        ];

        for layout in &[LayoutMode::Wide, LayoutMode::Narrow, LayoutMode::Stacked] {
            for sidebar_tab in &[SidebarTab::Files, SidebarTab::History] {
                for active_panel in &[Panel::Files, Panel::Editor, Panel::Response] {
                    state.layout_mode = *layout;
                    state.active_sidebar_tab = *sidebar_tab;
                    state.active_panel = *active_panel;

                    state.show_help = true;
                    state.show_unsaved_confirm = false;
                    terminal.draw(|f| {
                        render(f, &mut state);
                    }).unwrap();

                    state.show_help = false;
                    state.show_unsaved_confirm = true;
                    terminal.draw(|f| {
                        render(f, &mut state);
                    }).unwrap();
                }
            }
        }
    }
}
