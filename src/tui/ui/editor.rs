use crate::tui::state::{AppState, Panel};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
};

pub fn render(frame: &mut Frame, area: Rect, state: &AppState) {
    let focus = state.active_panel == Panel::Editor;
    let border_color = if focus { Color::Cyan } else { Color::DarkGray };

    // 计算总行数并渲染当前滚动行号
    let total_lines = state.editor_text.lines().count();
    let title = if total_lines > 0 {
        format!(
            " Request Preview [Line {}/{}] (Read-Only) | Run [Ctrl+Enter] ",
            state.editor_scroll + 1,
            total_lines
        )
    } else {
        " Request Preview [Line 0/0] (Read-Only) | Run [Ctrl+Enter] ".to_string()
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let paragraph = Paragraph::new(state.editor_text.as_str())
        .block(block)
        .scroll((state.editor_scroll as u16, 0));

    frame.render_widget(paragraph, area);
}
