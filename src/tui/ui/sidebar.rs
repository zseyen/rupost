use crate::tui::state::{AppState, SidebarTab};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs},
};

pub fn render(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let focus = state.active_panel == crate::tui::state::Panel::Files;
    let border_color = if focus { Color::Cyan } else { Color::DarkGray };

    // 侧边栏垂直拆分：1行用于 Tab 头部，其余用于列表内容
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(area);

    // 1. 渲染子 Tab 导航头
    let tab_titles = vec!["[F] Files", "[H] History"];
    let select_idx = match state.active_sidebar_tab {
        SidebarTab::Files => 0,
        SidebarTab::History => 1,
    };

    let tabs = Tabs::new(tab_titles)
        .select(select_idx)
        .style(Style::default().fg(Color::DarkGray))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(tabs, chunks[0]);

    // 2. 根据 Tab 内容切片渲染
    let viewport_height = chunks[1].height.saturating_sub(2) as usize; // 排除 Borders
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    match state.active_sidebar_tab {
        SidebarTab::Files => {
            let file_count = state.file_tree.len();
            state.files_state.clamp_scroll_offset(file_count, viewport_height);

            let start = state.files_state.scroll_offset;
            let end = (start + viewport_height).min(file_count);

            let lines: Vec<Line> = if state.file_tree.is_empty() {
                vec![Line::from(Span::styled(
                    "No files found in workspace.",
                    Style::default().fg(Color::DarkGray),
                ))]
            } else {
                state.file_tree[start..end]
                    .iter()
                    .enumerate()
                    .map(|(idx, full_path)| {
                        let actual_idx = start + idx;
                        let is_selected = actual_idx == state.files_state.selected_index;
                        let style = if is_selected {
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::REVERSED)
                        } else {
                            Style::default()
                        };

                        let display_text = if state.show_full_path {
                            full_path.clone()
                        } else {
                            std::path::Path::new(full_path)
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_else(|| full_path.clone())
                        };

                        // 如果当前文件已经加载进编辑器，显示一个小星号前缀
                        let is_loaded = actual_idx == state.loaded_file_index 
                            && state.editor_file_path.as_ref().map_or(false, |p| p == full_path);
                        let prefix = if is_loaded { "* " } else { "  " };

                        Line::from(vec![
                            Span::styled(prefix, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                            Span::styled(display_text, style),
                        ])
                    })
                    .collect()
            };

            frame.render_widget(Paragraph::new(lines).block(block.title(" Files ")), chunks[1]);
        }
        SidebarTab::History => {
            let history_count = state.history_list.len();
            state.history_state.clamp_scroll_offset(history_count, viewport_height);

            let start = state.history_state.scroll_offset;
            let end = (start + viewport_height).min(history_count);

            let lines: Vec<Line> = if state.history_list.is_empty() {
                vec![Line::from(Span::styled(
                    "No request history recorded.",
                    Style::default().fg(Color::DarkGray),
                ))]
            } else {
                state.history_list[start..end]
                    .iter()
                    .enumerate()
                    .map(|(idx, entry)| {
                        let actual_idx = start + idx;
                        let is_selected = actual_idx == state.history_state.selected_index;
                        let base_style = if is_selected {
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::REVERSED)
                        } else {
                            Style::default()
                        };

                        let method = &entry.request.method;
                        let method_color = match method.as_str() {
                            "GET" => Color::Cyan,
                            "POST" => Color::Yellow,
                            "PUT" => Color::Magenta,
                            "DELETE" => Color::Red,
                            _ => Color::White,
                        };

                        let status = entry.response.status;
                        let status_color = if status >= 200 && status < 400 {
                            Color::Green
                        } else {
                            Color::Red
                        };

                        let short_id = if entry.id.len() >= 8 {
                            &entry.id[..8]
                        } else {
                            &entry.id
                        };

                        let host_and_path = crate::tui::state::format_url_host_and_path(&entry.request.url);

                        Line::from(vec![
                            Span::styled(format!("[{}] ", short_id), Style::default().fg(Color::DarkGray)),
                            Span::styled(format!("{:<5}", method), Style::default().fg(method_color).add_modifier(Modifier::BOLD)),
                            Span::styled(format!(" {} ", host_and_path), base_style),
                            Span::styled(format!(" ({})", status), Style::default().fg(status_color)),
                        ])
                    })
                    .collect()
            };

            frame.render_widget(Paragraph::new(lines).block(block.title(" Request History ")), chunks[1]);
        }
    }
}
