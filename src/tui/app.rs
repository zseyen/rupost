use super::event::TuiEvent;
use super::state::AppState;
use crate::Result;
use crossterm::{
    event::{self, Event as CrosstermEvent},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::time::Duration;
use tokio::sync::mpsc;

/// 启动并运行 TUI 主事件循环
pub async fn run() -> Result<()> {
    run_async().await
}

async fn run_async() -> Result<()> {
    // 1. 初始化终端
    enable_raw_mode().map_err(crate::error::RupostError::IoError)?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(crate::error::RupostError::IoError)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)
        .map_err(|e| crate::error::RupostError::IoError(std::io::Error::other(e.to_string())))?;

    // 2. 建立 MPSC 事件通道
    let (event_tx, mut event_rx) = mpsc::channel(100);

    // 派发 crossterm 事件捕获 Task
    let tx_clone = event_tx.clone();
    tokio::spawn(async move {
        loop {
            // 阻断式轮询是否有 crossterm 事件
            if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                let event_res = event::read();
                if let Ok(CrosstermEvent::Key(key)) = event_res {
                    let send_res = tx_clone.send(TuiEvent::Input(key)).await;
                    if send_res.is_err() {
                        break;
                    }
                }
            }
            // 内部心跳
            if tx_clone.send(TuiEvent::Tick).await.is_err() {
                break;
            }
        }
    });

    let mut state = AppState::new();
    if let Ok(files) = crate::runner::scanner::DirectoryScanner::scan(&[".".to_string()]) {
        state.file_tree = files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
    }
    // 载入历史记录
    state.history_list = crate::history::storage::get_storage().tail(100).unwrap_or_default();
    state.history_list.reverse();

    let mut textarea = ratatui_textarea::TextArea::default();
    textarea.set_placeholder_text(
        "Press Tab to focus and type URL\nOr select a file on the left panel.",
    );

    // 默认加载首个文件
    if !state.file_tree.is_empty() {
        let first_file = &state.file_tree[0];
        if let Ok(content) = std::fs::read_to_string(first_file) {
            state.editor_text = content.clone();
            state.editor_file_path = Some(first_file.clone());
            textarea = ratatui_textarea::TextArea::new(content.lines().map(String::from).collect());
            state.loaded_file_index = 0;
        }
    }

    // 捕获初始尺寸
    if let Ok((w, h)) = crossterm::terminal::size() {
        state.update_layout(w, h);
    }

    // 3. 事件循环主流程
    loop {
        // A. 渲染当前状态
        terminal
            .draw(|frame| {
                super::ui::render(frame, &mut state, &mut textarea);
            })
            .map_err(|e| {
                crate::error::RupostError::IoError(std::io::Error::other(e.to_string()))
            })?;

        // B. 接收事件
        if let Some(event) = event_rx.recv().await {
            match event {
                TuiEvent::Input(key) => {
                    // 未保存强确认弹窗前置网关
                    if state.show_unsaved_confirm {
                        match key.code {
                            crossterm::event::KeyCode::Char('y')
                            | crossterm::event::KeyCode::Char('Y') => {
                                if let Some(action) = state.pending_action {
                                    match action {
                                        super::state::PendingAction::Quit => {
                                            state.is_quitting = true;
                                        }
                                        super::state::PendingAction::SwitchFile(idx) => {
                                            if idx < state.file_tree.len() {
                                                let path = &state.file_tree[idx];
                                                if let Ok(content) = std::fs::read_to_string(path) {
                                                    state.editor_text = content.clone();
                                                    state.editor_file_path = Some(path.clone());
                                                    textarea = ratatui_textarea::TextArea::new(
                                                        content.lines().map(String::from).collect(),
                                                    );
                                                    state.is_dirty = false;
                                                    state.loaded_file_index = idx;
                                                    state.selected_file_index = idx;
                                                    state.files_state.selected_index = idx;
                                                }
                                            }
                                        }
                                        super::state::PendingAction::SwitchHistory(idx) => {
                                            if idx < state.history_list.len() {
                                                let entry = &state.history_list[idx];
                                                let http_text = super::state::format_request_snapshot_to_http(&entry.request);
                                                state.editor_text = http_text.clone();
                                                state.editor_file_path = None;
                                                textarea = ratatui_textarea::TextArea::new(
                                                    http_text.lines().map(String::from).collect(),
                                                );
                                                state.is_dirty = true;
                                                state.history_state.selected_index = idx;
                                            }
                                        }
                                    }
                                }
                                state.show_unsaved_confirm = false;
                                state.pending_action = None;
                            }
                            crossterm::event::KeyCode::Char('n')
                            | crossterm::event::KeyCode::Char('N')
                            | crossterm::event::KeyCode::Esc => {
                                state.selected_file_index = state.loaded_file_index;
                                state.files_state.selected_index = state.loaded_file_index;
                                state.show_unsaved_confirm = false;
                                state.pending_action = None;
                            }
                            _ => {}
                        }
                        continue;
                    }

                    // 全局指令优先
                    if key.code == crossterm::event::KeyCode::Char('q') {
                        if state.is_dirty {
                            state.show_unsaved_confirm = true;
                            state.pending_action = Some(super::state::PendingAction::Quit);
                        } else {
                            state.update(super::event::Action::Quit);
                        }
                    }
                    if key.code == crossterm::event::KeyCode::Char('?') {
                        state.update(super::event::Action::ToggleHelp);
                    }
                    if key.code == crossterm::event::KeyCode::Tab {
                        let next_panel = match state.active_panel {
                            super::state::Panel::Files => super::state::Panel::Editor,
                            super::state::Panel::Editor => super::state::Panel::Response,
                            super::state::Panel::Response => super::state::Panel::Files,
                        };
                        state.update(super::event::Action::SwitchPanel(next_panel));
                    }
                    if key.code == crossterm::event::KeyCode::BackTab {
                        let prev_panel = match state.active_panel {
                            super::state::Panel::Response => super::state::Panel::Editor,
                            super::state::Panel::Editor => super::state::Panel::Files,
                            super::state::Panel::Files => super::state::Panel::Response,
                        };
                        state.update(super::event::Action::SwitchPanel(prev_panel));
                    }

                    // Files 面板操作 (已重构为双 Tab Sidebar)
                    if state.active_panel == super::state::Panel::Files && !state.show_help {
                        match key.code {
                            crossterm::event::KeyCode::Left
                            | crossterm::event::KeyCode::Char('h') => {
                                state.active_sidebar_tab = super::state::SidebarTab::Files;
                            }
                            crossterm::event::KeyCode::Right
                            | crossterm::event::KeyCode::Char('l') => {
                                state.active_sidebar_tab = super::state::SidebarTab::History;
                                load_history_response_to_state(&mut state);
                            }
                            crossterm::event::KeyCode::Char('p')
                            | crossterm::event::KeyCode::Char('P') => {
                                if state.active_sidebar_tab == super::state::SidebarTab::Files {
                                    state.show_full_path = !state.show_full_path;
                                }
                            }
                            crossterm::event::KeyCode::Down
                            | crossterm::event::KeyCode::Char('j') => {
                                match state.active_sidebar_tab {
                                    super::state::SidebarTab::Files => {
                                        if state.files_state.selected_index + 1 < state.file_tree.len() {
                                            state.files_state.selected_index += 1;
                                            state.selected_file_index = state.files_state.selected_index;
                                        }
                                    }
                                    super::state::SidebarTab::History => {
                                        if state.history_state.selected_index + 1 < state.history_list.len() {
                                            state.history_state.selected_index += 1;
                                            load_history_response_to_state(&mut state);
                                        }
                                    }
                                }
                            }
                            crossterm::event::KeyCode::Up
                            | crossterm::event::KeyCode::Char('k') => {
                                match state.active_sidebar_tab {
                                    super::state::SidebarTab::Files => {
                                        if state.files_state.selected_index > 0 {
                                            state.files_state.selected_index -= 1;
                                            state.selected_file_index = state.files_state.selected_index;
                                        }
                                    }
                                    super::state::SidebarTab::History => {
                                        if state.history_state.selected_index > 0 {
                                            state.history_state.selected_index -= 1;
                                            load_history_response_to_state(&mut state);
                                        }
                                    }
                                }
                            }
                            crossterm::event::KeyCode::Enter => {
                                match state.active_sidebar_tab {
                                    super::state::SidebarTab::Files => {
                                        if !state.file_tree.is_empty() {
                                            let path = &state.file_tree[state.files_state.selected_index];
                                            if state.is_dirty {
                                                if state.files_state.selected_index != state.loaded_file_index {
                                                    state.show_unsaved_confirm = true;
                                                    state.pending_action =
                                                        Some(super::state::PendingAction::SwitchFile(
                                                            state.files_state.selected_index,
                                                        ));
                                                }
                                            } else {
                                                if let Ok(content) = std::fs::read_to_string(path) {
                                                    state.editor_text = content.clone();
                                                    state.editor_file_path = Some(path.clone());
                                                    textarea = ratatui_textarea::TextArea::new(
                                                        content.lines().map(String::from).collect(),
                                                    );
                                                    state.is_dirty = false;
                                                    state.loaded_file_index = state.files_state.selected_index;
                                                }
                                            }
                                        }
                                    }
                                    super::state::SidebarTab::History => {
                                        if !state.history_list.is_empty() {
                                            let idx = state.history_state.selected_index;
                                            if state.is_dirty {
                                                state.show_unsaved_confirm = true;
                                                state.pending_action =
                                                    Some(super::state::PendingAction::SwitchHistory(idx));
                                            } else {
                                                let entry = &state.history_list[idx];
                                                let http_text = super::state::format_request_snapshot_to_http(&entry.request);
                                                state.editor_text = http_text.clone();
                                                state.editor_file_path = None;
                                                textarea = ratatui_textarea::TextArea::new(
                                                    http_text.lines().map(String::from).collect(),
                                                );
                                                state.is_dirty = true;
                                            }
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }

                    // Editor 面板操作
                    if state.active_panel == super::state::Panel::Editor && !state.show_help {
                        // Ctrl+R 或 Ctrl+Enter 触发异步运行
                        let is_run_key = (key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL)
                            && key.code == crossterm::event::KeyCode::Char('r'))
                            || (key
                                .modifiers
                                .contains(crossterm::event::KeyModifiers::CONTROL)
                                && key.code == crossterm::event::KeyCode::Enter);

                        if is_run_key {
                            if !state.is_loading {
                                let content = textarea.lines().join("\n");
                                match crate::parser::parse_content(&content) {
                                    Ok(parsed_file) => {
                                        let reqs = &parsed_file.requests;
                                        if !reqs.is_empty() {
                                            let req = reqs[0].clone();
                                            // 主动通知状态机，初始化相关变量，清空 Response 界面
                                            state.update(super::event::Action::SendRequest(Box::new(req.clone())));

                                            let mut var_context = state.variables.clone();
                                            var_context.insert("__default_scheme", "http");

                                            let (stream_tx, mut stream_rx) =
                                                tokio::sync::mpsc::unbounded_channel();

                                            let executor =
                                                crate::runner::TestExecutor::with_ephemeral_cookies()
                                                    .with_stream_sender(stream_tx);
                                            // 优雅回退 source 至 "tui" 标识
                                            let source = state.editor_file_path.clone().or_else(|| Some("tui".to_string()));
                                            let tx_clone = event_tx.clone();
                                            let req_id = uuid::Uuid::new_v4();

                                            let _ =
                                                tx_clone.send(TuiEvent::RequestStarted(req_id)).await;

                                            let s_tx = event_tx.clone();
                                            tokio::spawn(async move {
                                                while let Some(event) = stream_rx.recv().await {
                                                    match event {
                                                        crate::runner::types::StreamEvent::SseChunk(chunk) => {
                                                            let _ = s_tx.send(TuiEvent::StreamChunk { id: req_id, chunk, total_lines: 0 }).await;
                                                        }
                                                        crate::runner::types::StreamEvent::WsFrame { is_send, content } => {
                                                            let _ = s_tx.send(TuiEvent::WsFrame { id: req_id, is_send, content, total_lines: 0 }).await;
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
                                                let test_res = executor
                                                    .execute_one(req, 1, &mut var_context, source)
                                                    .await;
                                                
                                                // 写入请求历史
                                                if let Some(ref resp) = test_res.response {
                                                    let req_snapshot = crate::history::model::RequestSnapshot::from_parsed(&req_snap_arg);
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
                                                    Err(test_res.error.unwrap_or_else(|| {
                                                        "Unknown execution error".to_string()
                                                    }))
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
                                        } else {
                                            // 没找到请求
                                            state.last_response = Some(crate::http::Response::error("No request block found in editor.".to_string()));
                                            state.response_visual_lines.clear();
                                        }
                                    }
                                    Err(e) => {
                                        // 回显 HTTP 解析报错
                                        state.last_response = Some(crate::http::Response::error(format!("HTTP Parser Error: {}", e)));
                                        state.response_visual_lines.clear();
                                    }
                                }
                            }
                        } else if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL)
                            && key.code == crossterm::event::KeyCode::Char('s')
                        {
                            // Ctrl+S 保存
                            if let Some(ref path) = state.editor_file_path {
                                let text = textarea.lines().join("\n");
                                if std::fs::write(path, text).is_ok() {
                                    state.is_dirty = false;
                                }
                            }
                        } else if key.code != crossterm::event::KeyCode::Tab
                            && key.code != crossterm::event::KeyCode::BackTab
                            && key.code != crossterm::event::KeyCode::Char('?')
                        {
                            // 其余非全局功能键则派发给 textarea
                            textarea.input(key);
                            state.is_dirty = true;
                            state.editor_text = textarea.lines().join("\n");
                        }
                    }

                    // Response 面板操作
                    if state.active_panel == super::state::Panel::Response && !state.show_help {
                        match key.code {
                            crossterm::event::KeyCode::Up
                            | crossterm::event::KeyCode::Char('k') => {
                                state.response_scroll = state.response_scroll.saturating_sub(1);
                            }
                            crossterm::event::KeyCode::Down
                            | crossterm::event::KeyCode::Char('j') => {
                                state.response_scroll = state.response_scroll.saturating_add(1);
                            }
                            _ => {}
                        }
                    }
                }
                TuiEvent::Resize(w, h) => {
                    state.update_layout(w, h);
                }
                TuiEvent::RequestFinished {
                    result,
                    captured_vars,
                    assertions,
                    ..
                } => {
                    state.handle_request_finished(*result, captured_vars, assertions);
                    state.response_visual_lines.clear(); // 清空缓存以强迫重新计算预折行
                    state.response_scroll = 0;          // 请求结束重置滚动

                    // 从存储重新载入最新 100 条请求历史并倒序
                    state.history_list = crate::history::storage::get_storage().tail(100).unwrap_or_default();
                    state.history_list.reverse();
                    state.history_state.selected_index = 0;
                    state.history_state.scroll_offset = 0;
                }
                TuiEvent::StreamChunk { chunk, .. } => {
                    state.handle_stream_chunk(chunk);
                }
                TuiEvent::WsFrame {
                    is_send, content, ..
                } => {
                    state.handle_ws_frame(is_send, content);
                }
                TuiEvent::InitLogPath { path, .. } => {
                    state.log_file_path = Some(std::path::PathBuf::from(path));
                    state.total_log_lines = 0;
                    state.viewport_cache.clear();
                }
                TuiEvent::Tick => {}
                _ => {}
            }
        }

        if state.is_quitting {
            break;
        }
    }

    // 4. 清理并还原终端
    disable_raw_mode().map_err(crate::error::RupostError::IoError)?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .map_err(crate::error::RupostError::IoError)?;
    terminal
        .show_cursor()
        .map_err(|e| crate::error::RupostError::IoError(std::io::Error::other(e.to_string())))?;

    Ok(())
}

fn load_history_response_to_state(state: &mut crate::tui::state::AppState) {
    if state.active_sidebar_tab == crate::tui::state::SidebarTab::History 
        && !state.history_list.is_empty() 
        && state.history_state.selected_index < state.history_list.len() 
    {
        let entry = &state.history_list[state.history_state.selected_index];
        if let Ok(resp) = crate::http::Response::new(
            entry.response.status,
            entry.response.headers.clone(),
            entry.response.body.clone().unwrap_or_else(|| "Body not recorded".to_string()),
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
