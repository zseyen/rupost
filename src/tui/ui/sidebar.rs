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
            let file_count = state.visible_file_nodes.len();
            state
                .files_state
                .clamp_scroll_offset(file_count, viewport_height);

            let start = state.files_state.scroll_offset;
            let end = (start + viewport_height).min(file_count);

            let lines: Vec<Line> = if state.visible_file_nodes.is_empty() {
                vec![
                    Line::raw(""),
                    Line::from(Span::styled(
                        " (No HTTP files found in workspace)",
                        Style::default().fg(Color::DarkGray),
                    )),
                    Line::from(Span::styled(
                        " Press '?' for help",
                        Style::default().fg(Color::DarkGray),
                    )),
                ]
            } else {
                state.visible_file_nodes[start..end]
                    .iter()
                    .enumerate()
                    .map(|(idx, node)| {
                        let actual_idx = start + idx;
                        let is_selected = actual_idx == state.files_state.selected_index;

                        let base_style = if is_selected {
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::REVERSED)
                        } else {
                            Style::default()
                        };

                        // 1. 连线前缀 (使用淡灰色)
                        let prefix = node.render_prefix();

                        // 2. ▸ / ▾ 指示符
                        let indicator = if node.is_dir {
                            if state.expanded_dirs.contains(&node.rel_path) {
                                "▾ "
                            } else {
                                "▸ "
                            }
                        } else {
                            "  "
                        };

                        // 3. 文件名/目录名样式
                        let name_style = if node.is_dir {
                            base_style.add_modifier(Modifier::BOLD)
                        } else {
                            base_style
                        };

                        let is_loaded =
                            !node.is_dir && state.editor_file_path.as_ref() == Some(&node.abs_path);
                        let name_text = if is_loaded {
                            format!("{} *", node.display_name)
                        } else {
                            node.display_name.clone()
                        };

                        let mut spans = vec![
                            Span::styled(prefix, Style::default().fg(Color::DarkGray)),
                            Span::styled(indicator, Style::default().fg(Color::DarkGray)),
                            Span::styled(name_text, name_style),
                        ];

                        // 4. 执行状态 (如果是运行中，提供动态旋转 blind spinner，如果是完成，提供彩色 SUCCESS/FAILED)
                        if let Some(exec_state) = state.file_execution_states.get(&node.abs_path) {
                            use crate::tui::state::FileExecState;
                            match exec_state {
                                FileExecState::Running => {
                                    spans.push(Span::styled(
                                        format!(" [RUNNING] {}", state.get_spinner_char()),
                                        Style::default()
                                            .fg(Color::Yellow)
                                            .add_modifier(Modifier::BOLD),
                                    ));
                                }
                                FileExecState::Success => {
                                    spans.push(Span::styled(
                                        " [SUCCESS]",
                                        Style::default()
                                            .fg(Color::Green)
                                            .add_modifier(Modifier::BOLD),
                                    ));
                                }
                                FileExecState::Failed => {
                                    spans.push(Span::styled(
                                        " [FAILED]",
                                        Style::default()
                                            .fg(Color::Red)
                                            .add_modifier(Modifier::BOLD),
                                    ));
                                }
                            }
                        }

                        Line::from(spans)
                    })
                    .collect()
            };

            frame.render_widget(
                Paragraph::new(lines).block(block.title(" Files ")),
                chunks[1],
            );
        }
        SidebarTab::History => {
            let history_count = state.history_list.len();
            state
                .history_state
                .clamp_scroll_offset(history_count, viewport_height);

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
                        let status_color = if (200..400).contains(&status) {
                            Color::Green
                        } else {
                            Color::Red
                        };

                        let host_and_path =
                            crate::tui::state::format_url_host_and_path(&entry.request.url);

                        Line::from(vec![
                            Span::styled(
                                format!("{:<4}", method),
                                Style::default()
                                    .fg(method_color)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(format!(" {} ", host_and_path), base_style),
                            Span::styled(
                                format!(" ({})", status),
                                Style::default().fg(status_color),
                            ),
                        ])
                    })
                    .collect()
            };

            frame.render_widget(
                Paragraph::new(lines).block(block.title(" Request History ")),
                chunks[1],
            );
        }
    }
}
