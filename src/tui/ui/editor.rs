use crate::tui::state::{AppState, Panel};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders},
};

pub fn render(
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
