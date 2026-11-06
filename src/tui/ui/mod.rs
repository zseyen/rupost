pub mod editor;
pub mod popups;
pub mod response;
pub mod sidebar;
pub mod status_bar;

use super::state::{AppState, InputMode, LayoutMode, Panel};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    widgets::{Block, Borders, Paragraph, Tabs},
};

/// 渲染 TUI 面板布局主入口 (Orchestrator)
pub fn render(frame: &mut Frame, state: &mut AppState) {
    let t = state.palette();
    let size = frame.area();

    // 1. 终端超小防御机制
    if size.width < 40 || size.height < 10 {
        let warning = Paragraph::new("Terminal too small. Please enlarge the window.")
            .style(Style::default().fg(t.err).add_modifier(Modifier::BOLD))
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
            // 三栏并排 (25% sidebar, 40% editor, 35% response)
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
            // 三栏并排 (20% sidebar, 40% editor, 40% response)
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

            let titles = vec!["[1] Files", "[2] Editor", "[3] Response"];
            let active_idx = match state.active_panel {
                Panel::Files => 0,
                Panel::Editor => 1,
                Panel::Response => 2,
            };

            let tabs = Tabs::new(titles)
                .block(Block::default().borders(Borders::BOTTOM))
                .select(active_idx)
                .style(Style::default().fg(t.dim))
                .highlight_style(Style::default().fg(t.info).add_modifier(Modifier::BOLD));

            frame.render_widget(tabs, chunks[0]);

            match state.active_panel {
                Panel::Files => sidebar::render(frame, chunks[1], state),
                Panel::Editor => editor::render(frame, chunks[1], state),
                Panel::Response => response::render(frame, chunks[1], state),
            }
        }
    }

    // 3. 渲染全局快捷键帮助悬浮窗 (Modal Panel)
    if state.show_help {
        popups::render_help(frame, main_area, t);
    }

    // 4. 渲染未保存强确认弹窗 (Unsaved Changes Alert Modal)
    if state.show_unsaved_confirm {
        popups::render_unsaved_confirm(frame, main_area, t);
    }

    // 5. 渲染 X 键导出弹窗
    if let Some(cmd) = &state.show_export {
        popups::render_export(frame, main_area, cmd, t);
    }

    // 6. 渲染统一模态输入弹窗（快速新建 / 文件过滤 / 选择器 / AI断言等）
    if !matches!(state.input_mode, InputMode::None) {
        popups::render_modal_input(frame, main_area, state);
    }

    // 7. 渲染底部 lazygit 风格状态栏
    status_bar::render(frame, help_bar_area, state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::state::{AppState, InputMode, LayoutMode, Panel, SidebarTab};
    use crate::tui::theme::ThemeKind;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui_textarea::TextArea;

    #[test]
    fn smoke_test_components_render() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut state = AppState::new();
        state.raw_file_list = vec!["test1.http".to_string(), "subdir/test2.http".to_string()];
        state.rebuild_visible_tree_nodes();
        state.history_list = vec![crate::history::model::HistoryEntry {
            id: "1".to_string(),
            timestamp: chrono::Utc::now(),
            duration_ms: 15,
            request: crate::history::model::RequestSnapshot {
                timeout: None,
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
        }];

        for theme in &[ThemeKind::Dark, ThemeKind::Light] {
            state.theme = *theme;
            for last_error in &[None, Some("create failed: permission denied".to_string())] {
                state.last_error = last_error.clone();
                for input_mode in &[0u8, 1, 2] {
                    state.input_mode = match input_mode {
                        1 => InputMode::QuickCreate(Box::new(TextArea::default())),
                        2 => InputMode::Filter(Box::new(TextArea::default())),
                        _ => InputMode::None,
                    };
                    for layout in &[LayoutMode::Wide, LayoutMode::Narrow, LayoutMode::Stacked] {
                        for sidebar_tab in &[SidebarTab::Files, SidebarTab::History] {
                            for active_panel in &[Panel::Files, Panel::Editor, Panel::Response] {
                                state.layout_mode = *layout;
                                state.active_sidebar_tab = *sidebar_tab;
                                state.active_panel = *active_panel;

                                state.show_help = true;
                                state.show_unsaved_confirm = false;
                                terminal
                                    .draw(|f| {
                                        render(f, &mut state);
                                    })
                                    .unwrap();

                                state.show_help = false;
                                state.show_unsaved_confirm = true;
                                terminal
                                    .draw(|f| {
                                        render(f, &mut state);
                                    })
                                    .unwrap();

                                // 验证导出弹窗渲染
                                state.show_unsaved_confirm = false;
                                state.show_export = Some("curl http://example.com".to_string());
                                terminal
                                    .draw(|f| {
                                        render(f, &mut state);
                                    })
                                    .unwrap();
                                state.show_export = None;
                            }
                        }
                    }
                }
            }
        }
    }
}
