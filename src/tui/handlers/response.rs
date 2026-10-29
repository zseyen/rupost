//! Response 面板组件：滚动、详情子选项卡切换与消息搜索（`/`、n/N）。

use super::nav;
use crate::tui::state::{AppState, DetailTab, InputMode, Panel};
use crossterm::event::{KeyCode, KeyEvent};

/// 处理 Response 面板键盘滚动、搜索入口与 n/N 跳转
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
        KeyCode::Char('/') => state.open_search(),
        // n/N：query 留存（Search 模态退出后仍可用），环绕跳转
        KeyCode::Char('n') => state.jump_to_match(true),
        KeyCode::Char('N') => state.jump_to_match(false),
        _ => {}
    }
}

/// 详情区内容总行数——与渲染分支的视觉数据源一致（R5 修正：
/// 此前 WS 计帧数、SSE 计物理行，均与 wrap 后视觉行数不一致，滚动钳制失准）。
/// 流式态 = 视觉行缓存长度；完成态长连接 = 日志总行数（滑窗锚点语义）；HTTP = 视觉行。
pub fn response_total_lines(state: &AppState) -> usize {
    if state.active_detail_tab == DetailTab::Response {
        let is_ws = !state.ws_frames.is_empty()
            || state
                .current_request
                .as_ref()
                .map(|r| r.metadata.websocket)
                .unwrap_or(false);
        let is_sse = !state.sse_stream_body.is_empty();
        if is_ws || is_sse {
            if state.is_loading {
                if is_ws {
                    state.ws_visual_lines.len()
                } else {
                    state.sse_visual_lines.len()
                }
            } else {
                state.total_log_lines
            }
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

/// 供搜索跳转测试使用：确认 Response 面板分发链包含搜索入口
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::event::TuiEvent;
    use crossterm::event::KeyModifiers;

    #[tokio::test]
    async fn test_slash_opens_search_modal_in_response_panel() {
        let mut state = AppState::new();
        state.active_panel = Panel::Response;
        let (tx, _rx) = tokio::sync::mpsc::channel::<TuiEvent>(1);
        let key = KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE);
        crate::tui::handlers::handle_key(&mut state, key, &tx).await;
        assert!(matches!(state.input_mode, InputMode::Search(_)));
    }

    #[test]
    fn test_total_lines_matches_visual_sources() {
        let mut state = AppState::new();
        // HTTP 完成态：视觉行数
        state.response_visual_lines = vec![ratatui::text::Line::raw("l"); 7];
        assert_eq!(response_total_lines(&state), 7);
        // SSE 流式态：sse_visual_lines（而非物理行数）
        state.is_loading = true;
        state.sse_stream_body = "a\nb\nc\nd\n".to_string(); // 4 物理行
        state.sse_visual_lines = vec![ratatui::text::Line::raw("v"); 9]; // wrap 后 9 视觉行
        assert_eq!(response_total_lines(&state), 9);
        // WS 流式态：ws_visual_lines
        state.ws_frames.push(crate::tui::state::WsFrameRecord {
            is_send: true,
            timestamp: "12:00:00".to_string(),
            content: "PING".to_string(),
        });
        state.ws_visual_lines = vec![ratatui::text::Line::raw("w"); 3];
        assert_eq!(response_total_lines(&state), 3);
        // 完成态长连接：日志总行数
        state.is_loading = false;
        state.total_log_lines = 500;
        assert_eq!(response_total_lines(&state), 500);
    }
}
