use super::event::{Action, TuiEvent};
use super::state::{AppState, Panel, PendingAction, SidebarTab};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use tokio::sync::mpsc;

/// 处理 TUI 按键输入事件
pub async fn handle_key(state: &mut AppState, key: KeyEvent, event_tx: &mpsc::Sender<TuiEvent>) {
    // 1. 未保存确认弹窗的前置处理
    if state.show_unsaved_confirm {
        handle_confirm_key(state, key);
        return;
    }

    // 全局 Esc 键高优先级拦截（用于关闭其他全局轻量弹窗并返回 Files 面板）
    if key.code == KeyCode::Esc {
        if state.show_help {
            state.show_help = false;
            return;
        }
        if state.active_panel != Panel::Files {
            state.active_panel = Panel::Files;
            return;
        }
    }

    // 2. 全局高优先级指令
    if key.code == KeyCode::Char('q') {
        state.update(Action::Quit);
        return;
    }
    if key.code == KeyCode::Char('?') {
        state.update(Action::ToggleHelp);
        return;
    }
    if key.code == KeyCode::Tab {
        let next_panel = match state.active_panel {
            Panel::Files => Panel::Editor,
            Panel::Editor => Panel::Response,
            Panel::Response => Panel::Files,
        };
        state.update(Action::SwitchPanel(next_panel));
        return;
    }
    if key.code == KeyCode::BackTab {
        let prev_panel = match state.active_panel {
            Panel::Response => Panel::Editor,
            Panel::Editor => Panel::Files,
            Panel::Files => Panel::Response,
        };
        state.update(Action::SwitchPanel(prev_panel));
        return;
    }

    // 3. 根据当前激活的面板分发特定的键盘操作
    match state.active_panel {
        Panel::Files => handle_sidebar_key(state, key),
        Panel::Editor => handle_editor_key(state, key, event_tx).await,
        Panel::Response => handle_response_key(state, key),
    }
}

/// 处理未保存改动强确认弹窗的按键
fn handle_confirm_key(state: &mut AppState, key: KeyEvent) {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            if let Some(action) = state.pending_action {
                match action {
                    PendingAction::Quit => {
                        state.is_quitting = true;
                    }
                    PendingAction::SwitchFile(idx) => {
                        if idx < state.visible_file_nodes.len() {
                            let node = &state.visible_file_nodes[idx];
                            if !node.is_dir
                                && let Ok(content) = std::fs::read_to_string(&node.abs_path)
                            {
                                state.editor_text = content;
                                state.editor_file_path = Some(node.abs_path.clone());
                                state.is_dirty = false;
                                state.loaded_file_index = idx;
                                state.selected_file_index = idx;
                                state.files_state.selected_index = idx;
                                state.editor_scroll = 0;
                                state.active_panel = Panel::Editor;
                            }
                        }
                    }
                    PendingAction::SwitchHistory(idx) => {
                        if idx < state.history_list.len() {
                            state.history_state.selected_index = idx;
                            let entry = &state.history_list[idx];
                            let http_text =
                                super::state::format_request_snapshot_to_http(&entry.request);
                            state.editor_text = http_text;
                            state.editor_file_path = None;
                            state.is_dirty = false;
                            load_history_response_to_state(state);
                            state.editor_scroll = 0;
                            state.active_panel = Panel::Editor;
                        }
                    }
                }
            }
            state.show_unsaved_confirm = false;
            state.pending_action = None;
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            state.selected_file_index = state.loaded_file_index;
            state.files_state.selected_index = state.loaded_file_index;
            state.show_unsaved_confirm = false;
            state.pending_action = None;
        }
        _ => {}
    }
}

/// 处理侧边栏面板（文件树和历史记录 Tab）的按键
fn handle_sidebar_key(state: &mut AppState, key: KeyEvent) {
    if state.show_help {
        return;
    }
    match key.code {
        KeyCode::Left | KeyCode::Char('h') => {
            if state.active_sidebar_tab == SidebarTab::Files && !state.visible_file_nodes.is_empty()
            {
                let idx = state.files_state.selected_index;
                if idx < state.visible_file_nodes.len() {
                    let node = &state.visible_file_nodes[idx];
                    if node.is_dir && state.expanded_dirs.contains(&node.rel_path) {
                        state.toggle_directory(idx);
                        return;
                    }
                }
            }
            state.active_sidebar_tab = SidebarTab::Files;
            sync_preview_to_editor(state);
        }
        KeyCode::Right | KeyCode::Char('l') => {
            if state.active_sidebar_tab == SidebarTab::Files && !state.visible_file_nodes.is_empty()
            {
                let idx = state.files_state.selected_index;
                if idx < state.visible_file_nodes.len() {
                    let node = &state.visible_file_nodes[idx];
                    if node.is_dir && !state.expanded_dirs.contains(&node.rel_path) {
                        state.toggle_directory(idx);
                        return;
                    }
                }
            }
            state.active_sidebar_tab = SidebarTab::History;
            sync_preview_to_editor(state);
        }
        KeyCode::Char('p') | KeyCode::Char('P') => {
            if state.active_sidebar_tab == SidebarTab::Files {
                state.show_full_path = !state.show_full_path;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => match state.active_sidebar_tab {
            SidebarTab::Files => {
                if state.files_state.selected_index + 1 < state.visible_file_nodes.len() {
                    state.files_state.selected_index += 1;
                    state.selected_file_index = state.files_state.selected_index;
                    sync_preview_to_editor(state);
                }
            }
            SidebarTab::History => {
                if state.history_state.selected_index + 1 < state.history_list.len() {
                    state.history_state.selected_index += 1;
                    sync_preview_to_editor(state);
                }
            }
        },
        KeyCode::Up | KeyCode::Char('k') => match state.active_sidebar_tab {
            SidebarTab::Files => {
                if state.files_state.selected_index > 0 {
                    state.files_state.selected_index -= 1;
                    state.selected_file_index = state.files_state.selected_index;
                    sync_preview_to_editor(state);
                }
            }
            SidebarTab::History => {
                if state.history_state.selected_index > 0 {
                    state.history_state.selected_index -= 1;
                    sync_preview_to_editor(state);
                }
            }
        },
        KeyCode::Enter => match state.active_sidebar_tab {
            SidebarTab::Files => {
                if !state.visible_file_nodes.is_empty() {
                    let idx = state.files_state.selected_index;
                    if idx < state.visible_file_nodes.len() {
                        let node = &state.visible_file_nodes[idx];
                        if node.is_dir {
                            state.toggle_directory(idx);
                        } else {
                            state.active_panel = Panel::Editor;
                        }
                    }
                }
            }
            SidebarTab::History => {
                if !state.history_list.is_empty() {
                    state.active_panel = Panel::Editor;
                }
            }
        },
        _ => {}
    }
}

/// 处理编辑器面板按键（包括触发运行测试请求）
async fn handle_editor_key(state: &mut AppState, key: KeyEvent, event_tx: &mpsc::Sender<TuiEvent>) {
    if state.show_help {
        return;
    }
    let is_run_key = (key.modifiers.contains(KeyModifiers::CONTROL)
        && key.code == KeyCode::Char('r'))
        || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Enter);
    let total_lines = state.editor_text.lines().count();

    if is_run_key {
        if state.is_loading {
            return;
        }
        if !state.is_loading {
            let parsed_req = if state.active_sidebar_tab == SidebarTab::History {
                state.current_request.clone()
            } else {
                match crate::parser::parse_content(&state.editor_text) {
                    Ok(parsed) => parsed.requests.first().cloned(),
                    Err(e) => {
                        state.last_response = Some(crate::http::Response::error(format!(
                            "HTTP Parser Error: {}",
                            e
                        )));
                        state.response_visual_lines.clear();
                        None
                    }
                }
            };

            if let Some(req) = parsed_req {
                let mut var_context = state.variables.clone();
                var_context.insert("__default_scheme", "http");

                let (stream_tx, mut stream_rx) = mpsc::unbounded_channel();
                let executor = crate::runner::TestExecutor::with_ephemeral_cookies()
                    .with_stream_sender(stream_tx);
                let source = state
                    .editor_file_path
                    .clone()
                    .or_else(|| Some("tui".to_string()));
                let tx_clone = event_tx.clone();
                let req_id = uuid::Uuid::new_v4();

                let _ = tx_clone.send(TuiEvent::RequestStarted(req_id)).await;

                let s_tx = event_tx.clone();
                tokio::spawn(async move {
                    while let Some(event) = stream_rx.recv().await {
                        match event {
                            crate::runner::types::StreamEvent::SseChunk(chunk) => {
                                let _ = s_tx
                                    .send(TuiEvent::StreamChunk {
                                        id: req_id,
                                        chunk,
                                        total_lines: 0,
                                    })
                                    .await;
                            }
                            crate::runner::types::StreamEvent::WsFrame { is_send, content } => {
                                let _ = s_tx
                                    .send(TuiEvent::WsFrame {
                                        id: req_id,
                                        is_send,
                                        content,
                                        total_lines: 0,
                                    })
                                    .await;
                            }
                            crate::runner::types::StreamEvent::InitLogPath(path) => {
                                let _ = s_tx.send(TuiEvent::InitLogPath { id: req_id, path }).await;
                            }
                        }
                    }
                });

                let req_snap_arg = req.clone();
                let source_clone = source.clone();
                tokio::spawn(async move {
                    let test_res = executor.execute_one(req, 1, &mut var_context, source).await;

                    if let Some(ref resp) = test_res.response {
                        let req_snapshot =
                            crate::history::model::RequestSnapshot::from_parsed(&req_snap_arg);
                        crate::history::recorder::record_history(req_snapshot, resp, source_clone);
                    }

                    let captured_vars = var_context.variables().clone();
                    let result = if test_res.success {
                        if let Some(resp) = test_res.response {
                            Ok(resp)
                        } else {
                            Err("Request succeeded but no response returned".to_string())
                        }
                    } else {
                        Err(test_res
                            .error
                            .unwrap_or_else(|| "Unknown execution error".to_string()))
                    };

                    let _ = tx_clone
                        .send(TuiEvent::RequestFinished {
                            id: req_id,
                            result: Box::new(result),
                            captured_vars,
                            assertions: test_res.assertions,
                        })
                        .await;
                });
            } else if state.last_response.is_none() {
                state.last_response = Some(crate::http::Response::error(
                    "No request block found in editor.".to_string(),
                ));
                state.response_visual_lines.clear();
            }
        }
    } else {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                state.editor_scroll = state.editor_scroll.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if state.editor_scroll + 1 < total_lines {
                    state.editor_scroll += 1;
                }
            }
            KeyCode::PageUp => {
                state.editor_scroll = state.editor_scroll.saturating_sub(10);
            }
            KeyCode::PageDown => {
                state.editor_scroll = (state.editor_scroll + 10).min(total_lines.saturating_sub(1));
            }
            _ => {}
        }
    }
}

/// 处理 Response 面板键盘滚动操作
fn handle_response_key(state: &mut AppState, key: KeyEvent) {
    if state.show_help {
        return;
    }
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            state.response_scroll = state.response_scroll.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let visible_height = state.terminal_height.saturating_sub(2) as usize;
            let total = if !state.ws_frames.is_empty() {
                state.ws_frames.len()
            } else if !state.sse_stream_body.is_empty() {
                state.sse_stream_body.lines().count()
            } else {
                state.response_visual_lines.len()
            };
            let max_scroll = total.saturating_sub(visible_height);
            if state.response_scroll < max_scroll {
                state.response_scroll = state.response_scroll.saturating_add(1);
            }
        }
        _ => {}
    }
}

/// 处理鼠标点击坐标映射与面板切换
pub fn handle_mouse(state: &mut AppState, mouse_event: MouseEvent) {
    if mouse_event.kind == MouseEventKind::Down(MouseButton::Left) {
        let x = mouse_event.column;
        let y = mouse_event.row;
        let w = state.terminal_width;
        let h = state.terminal_height;

        match state.layout_mode {
            super::state::LayoutMode::Wide => {
                let sidebar_w = (w as f32 * 0.25) as u16;
                let editor_w = (w as f32 * 0.40) as u16;
                if y == 0 && x < sidebar_w {
                    state.active_panel = Panel::Files;
                    if x < 11 {
                        state.active_sidebar_tab = SidebarTab::Files;
                    } else if (12..24).contains(&x) {
                        state.active_sidebar_tab = SidebarTab::History;
                    }
                    sync_preview_to_editor(state);
                } else if y >= 1 && y < h - 1 && x < sidebar_w {
                    state.active_panel = Panel::Files;
                    let click_row = y.saturating_sub(2) as usize;
                    match state.active_sidebar_tab {
                        SidebarTab::Files => {
                            let idx = state.files_state.scroll_offset + click_row;
                            if idx < state.visible_file_nodes.len() {
                                let is_dir = state.visible_file_nodes[idx].is_dir;
                                if is_dir {
                                    state.toggle_directory(idx);
                                } else {
                                    state.files_state.selected_index = idx;
                                    state.selected_file_index = idx;
                                    sync_preview_to_editor(state);
                                }
                            }
                        }
                        SidebarTab::History => {
                            let idx = state.history_state.scroll_offset + click_row;
                            if idx < state.history_list.len() {
                                state.history_state.selected_index = idx;
                                sync_preview_to_editor(state);
                            }
                        }
                    }
                } else if x >= sidebar_w && x < sidebar_w + editor_w && y < h - 1 {
                    state.active_panel = Panel::Editor;
                } else if x >= sidebar_w + editor_w && x < w && y < h - 1 {
                    state.active_panel = Panel::Response;
                }
            }
            super::state::LayoutMode::Narrow => {
                let sidebar_w = (w as f32 * 0.20) as u16;
                let editor_w = (w as f32 * 0.40) as u16;
                if y == 0 && x < sidebar_w {
                    state.active_panel = Panel::Files;
                    if x < 11 {
                        state.active_sidebar_tab = SidebarTab::Files;
                    } else if (12..24).contains(&x) {
                        state.active_sidebar_tab = SidebarTab::History;
                    }
                    sync_preview_to_editor(state);
                } else if y >= 1 && y < h - 1 && x < sidebar_w {
                    state.active_panel = Panel::Files;
                    let click_row = y.saturating_sub(2) as usize;
                    match state.active_sidebar_tab {
                        SidebarTab::Files => {
                            let idx = state.files_state.scroll_offset + click_row;
                            if idx < state.visible_file_nodes.len() {
                                let is_dir = state.visible_file_nodes[idx].is_dir;
                                if is_dir {
                                    state.toggle_directory(idx);
                                } else {
                                    state.files_state.selected_index = idx;
                                    state.selected_file_index = idx;
                                    sync_preview_to_editor(state);
                                }
                            }
                        }
                        SidebarTab::History => {
                            let idx = state.history_state.scroll_offset + click_row;
                            if idx < state.history_list.len() {
                                state.history_state.selected_index = idx;
                                sync_preview_to_editor(state);
                            }
                        }
                    }
                } else if x >= sidebar_w && x < sidebar_w + editor_w && y < h - 1 {
                    state.active_panel = Panel::Editor;
                } else if x >= sidebar_w + editor_w && x < w && y < h - 1 {
                    state.active_panel = Panel::Response;
                }
            }
            super::state::LayoutMode::Stacked => {
                if y <= 2 {
                    let col_w = w / 3;
                    if x < col_w {
                        state.active_panel = Panel::Files;
                    } else if x >= col_w && x < col_w * 2 {
                        state.active_panel = Panel::Editor;
                    } else {
                        state.active_panel = Panel::Response;
                    }
                } else if y > 2 && y < h - 1 && state.active_panel == Panel::Files {
                    if y == 3 {
                        if x < 11 {
                            state.active_sidebar_tab = SidebarTab::Files;
                        } else if (12..24).contains(&x) {
                            state.active_sidebar_tab = SidebarTab::History;
                        }
                        sync_preview_to_editor(state);
                    } else if y >= 4 {
                        let click_row = y.saturating_sub(5) as usize;
                        match state.active_sidebar_tab {
                            SidebarTab::Files => {
                                let idx = state.files_state.scroll_offset + click_row;
                                if idx < state.visible_file_nodes.len() {
                                    let is_dir = state.visible_file_nodes[idx].is_dir;
                                    if is_dir {
                                        state.toggle_directory(idx);
                                    } else {
                                        state.files_state.selected_index = idx;
                                        state.selected_file_index = idx;
                                        sync_preview_to_editor(state);
                                    }
                                }
                            }
                            SidebarTab::History => {
                                let idx = state.history_state.scroll_offset + click_row;
                                if idx < state.history_list.len() {
                                    state.history_state.selected_index = idx;
                                    sync_preview_to_editor(state);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ==========================================
// 辅助状态同步函数 (交互内部使用)
// ==========================================

fn load_history_response_to_state(state: &mut AppState) {
    if !state.history_list.is_empty()
        && state.history_state.selected_index < state.history_list.len()
    {
        let entry = &state.history_list[state.history_state.selected_index];
        if let Ok(resp) = crate::http::Response::new(
            entry.response.status,
            entry.response.headers.clone(),
            entry
                .response
                .body
                .clone()
                .unwrap_or_else(|| "Body not recorded".to_string()),
            std::time::Duration::from_millis(entry.duration_ms),
            std::time::Duration::ZERO,
            std::time::Duration::ZERO,
        ) {
            state.last_response = Some(resp);
            state.response_visual_lines.clear();
            state.response_scroll = 0;
        }
    }
}

fn sync_preview_to_editor(state: &mut AppState) {
    if state.is_dirty {
        return;
    }
    match state.active_sidebar_tab {
        SidebarTab::Files => {
            if !state.visible_file_nodes.is_empty()
                && state.files_state.selected_index < state.visible_file_nodes.len()
            {
                let node = &state.visible_file_nodes[state.files_state.selected_index];
                if !node.is_dir
                    && let Ok(content) = std::fs::read_to_string(&node.abs_path)
                {
                    state.editor_text = content;
                    state.editor_file_path = Some(node.abs_path.clone());
                    if let Ok(metadata) = std::fs::metadata(&node.abs_path) {
                        state.editor_file_mtime = metadata.modified().ok();
                    } else {
                        state.editor_file_mtime = None;
                    }
                    state.is_dirty = false;
                    state.loaded_file_index = state.files_state.selected_index;
                    state.editor_scroll = 0;
                }
            }
        }
        SidebarTab::History => {
            if !state.history_list.is_empty()
                && state.history_state.selected_index < state.history_list.len()
            {
                let entry = &state.history_list[state.history_state.selected_index];
                let http_text = super::state::format_request_snapshot_to_http(&entry.request);
                state.editor_text = http_text;
                state.editor_file_path = None;
                state.editor_file_mtime = None;
                state.is_dirty = false;
                load_history_response_to_state(state);
                state.editor_scroll = 0;
            }
        }
    }
}

// ==========================================
// 单元测试部分
// ==========================================

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[tokio::test]
    async fn test_handle_global_tab_navigation() {
        let mut state = AppState::new();
        let (tx, _rx) = mpsc::channel(1);

        assert_eq!(state.active_panel, Panel::Files);

        // 模拟 Tab 键
        let tab_key = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
        handle_key(&mut state, tab_key, &tx).await;
        assert_eq!(state.active_panel, Panel::Editor);

        // 再次 Tab 键
        handle_key(&mut state, tab_key, &tx).await;
        assert_eq!(state.active_panel, Panel::Response);

        // 模拟 Shift+Tab (BackTab) 键
        let back_tab_key = KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE);
        handle_key(&mut state, back_tab_key, &tx).await;
        assert_eq!(state.active_panel, Panel::Editor);
    }

    #[tokio::test]
    async fn test_unsaved_confirm_dialog_gate() {
        let mut state = AppState::new();
        let (tx, _rx) = mpsc::channel(1);

        state.show_unsaved_confirm = true;
        state.pending_action = Some(PendingAction::Quit);

        // 模拟按 'N' 取消
        let key_n = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE);
        handle_key(&mut state, key_n, &tx).await;
        assert!(!state.show_unsaved_confirm);
        assert_eq!(state.pending_action, None);
        assert!(!state.is_quitting);

        // 重设为确认
        state.show_unsaved_confirm = true;
        state.pending_action = Some(PendingAction::Quit);
        // 模拟按 'Y' 确认退出
        let key_y = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE);
        handle_key(&mut state, key_y, &tx).await;
        assert!(!state.show_unsaved_confirm);
        assert_eq!(state.pending_action, None);
        assert!(state.is_quitting);
    }

    #[tokio::test]
    async fn test_response_scrolling_limits() {
        let mut state = AppState::new();
        state.active_panel = Panel::Response;
        state.terminal_height = 10; // 可见高度为 10-2 = 8 行

        // 模拟响应折行行数为 12 行
        state.response_visual_lines = vec![ratatui::text::Line::raw("line"); 12];

        let (tx, _rx) = mpsc::channel(1);

        // 初始滚动为 0
        assert_eq!(state.response_scroll, 0);

        // 模拟向下滚动，最大可以滚动到 12 - 8 = 4
        let down_key = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        for _ in 0..10 {
            handle_key(&mut state, down_key, &tx).await;
        }
        assert_eq!(state.response_scroll, 4);

        // 模拟向上滚动
        let up_key = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        handle_key(&mut state, up_key, &tx).await;
        assert_eq!(state.response_scroll, 3);
    }

    #[tokio::test]
    async fn test_mouse_clicking_panel_navigation() {
        let mut state = AppState::new();
        state.terminal_width = 120;
        state.terminal_height = 30;
        state.layout_mode = super::super::state::LayoutMode::Wide; // 宽屏布局

        assert_eq!(state.active_panel, Panel::Files);

        // 鼠标点击 Editor 区域 (x = 40, y = 5)
        let click_editor = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 40,
            row: 5,
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse(&mut state, click_editor);
        assert_eq!(state.active_panel, Panel::Editor);

        // 鼠标点击 Response 区域 (x = 90, y = 5)
        let click_response = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 90,
            row: 5,
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse(&mut state, click_response);
        assert_eq!(state.active_panel, Panel::Response);
    }
}
