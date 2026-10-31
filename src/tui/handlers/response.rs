//! Response 面板组件：滚动与详情子选项卡切换。

use super::nav;
use crate::tui::state::{AppState, DetailTab};
use crossterm::event::{KeyCode, KeyEvent};

/// 处理 Response 面板键盘滚动操作
pub(super) fn handle_response_key(state: &mut AppState, key: KeyEvent) {
    if state.show_help {
        return;
    }
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            state.response_scroll = nav::scroll_up(state.response_scroll, 1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let viewport = state.terminal_height.saturating_sub(2) as usize;
            let max_offset = nav::response_max_offset(response_total_lines(state), viewport);
            state.response_scroll = nav::scroll_down(state.response_scroll, max_offset, 1);
        }
        KeyCode::Left | KeyCode::Char('h') => switch_detail_tab(state, false),
        KeyCode::Right | KeyCode::Char('l') => switch_detail_tab(state, true),
        _ => {}
    }
}

/// 详情区内容总行数（长连接帧优先于流式正文，再退回静态视觉行）
pub fn response_total_lines(state: &AppState) -> usize {
    if state.active_detail_tab == DetailTab::Response {
        if !state.ws_frames.is_empty() {
            state.ws_frames.len()
        } else if !state.sse_stream_body.is_empty() {
            state.sse_stream_body.lines().count()
        } else {
            state.response_visual_lines.len()
        }
    } else {
        state.response_visual_lines.len()
    }
}

fn switch_detail_tab(state: &mut AppState, forward: bool) {
    state.active_detail_tab = nav::next_detail_tab(state.active_detail_tab, forward);
    state.response_visual_lines.clear();
    state.response_scroll = 0;
}
