//! Response 面板组件：滚动、详情子选项卡切换与消息搜索（`/`、n/N）。

use super::nav;
use crate::tui::state::{AppState, DetailTab};
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
    use crate::tui::state::{InputMode, Panel};
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

#[cfg(test)]
mod search_flow_tests {
    use super::*;
    use crate::tui::event::TuiEvent;
    use crate::tui::handlers::handle_key;
    use crate::tui::state::{InputMode, Panel};
    use crossterm::event::KeyModifiers;
    use tokio::sync::mpsc;

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn state_with_lines(lines: &[&str]) -> AppState {
        let mut state = AppState::new();
        state.active_panel = Panel::Response;
        state.terminal_height = 20;
        state.response_visual_lines = lines
            .iter()
            .map(|l| ratatui::text::Line::raw(l.to_string()))
            .collect();
        state
    }

    /// R4 守护：`/` 模态逐键输入实时更新 query（克隆 Filter 的 live 语义）
    #[tokio::test]
    async fn test_search_modal_live_typing_updates_query() {
        let mut state = state_with_lines(&["alpha", "beta"]);
        let (tx, _rx) = mpsc::channel::<TuiEvent>(1);

        handle_key(&mut state, key('/'), &tx).await;
        assert!(matches!(state.input_mode, InputMode::Search(_)));

        for c in "al".chars() {
            handle_key(&mut state, key(c), &tx).await;
        }
        assert_eq!(state.search_query, "al", "query must update live");

        // Esc 退出：query 留存
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &tx,
        )
        .await;
        assert!(matches!(state.input_mode, InputMode::None));
        assert_eq!(state.search_query, "al");

        // 留存 query 下 n/N 续跳
        state.jump_to_match(true);
        assert_eq!(state.response_scroll, 0);
    }

    /// R5 守护：完成态长连接视图 Down 滚动按 total_log_lines 钳制
    /// （旧计数 bug 会按错误基数提前停或越界）
    #[test]
    fn test_completed_scroll_clamps_to_total_log_lines() {
        let mut state = AppState::new();
        state.active_panel = Panel::Response;
        state.is_loading = false;
        state.sse_stream_body = "legacy".to_string();
        state.total_log_lines = 150;
        state.terminal_height = 20;

        for _ in 0..200 {
            handle_response_key(&mut state, key('j'));
        }
        // max = 150 - (20-2) = 132
        assert_eq!(state.response_scroll, 132, "must clamp to total-viewport");
    }

    /// R4 守护：完成态 SSE 的搜索空间同为日志文件（与 WS 对称）
    #[test]
    fn test_search_space_sse_completed_uses_log_file() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("sse.log");
        std::fs::write(&log, "chunk-0\nchunk-1\n").unwrap();

        let mut state = AppState::new();
        state.active_detail_tab = crate::tui::state::DetailTab::Response;
        state.sse_stream_body = "chunk-0\n".to_string();
        state.is_loading = false;
        state.log_file_path = Some(log.clone());
        state.search_query = "chunk-1".to_string();

        match state.search_space() {
            crate::tui::state::search::SearchSpace::LogFile(p) => assert_eq!(p, log),
            other => panic!("expected LogFile space, got {other:?}"),
        }
        state.jump_to_match(true);
        assert_eq!(state.response_scroll, 1);
    }
}
