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
pub async fn run(initial_file: Option<String>) -> Result<()> {
    run_async(initial_file).await
}

async fn run_async(initial_file: Option<String>) -> Result<()> {
    // 1. 初始化终端
    enable_raw_mode().map_err(crate::error::RupostError::IoError)?;
    let mut stdout = std::io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )
    .map_err(crate::error::RupostError::IoError)?;

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
                match event::read() {
                    Ok(CrosstermEvent::Key(key)) => {
                        if tx_clone.send(TuiEvent::Input(key)).await.is_err() {
                            break;
                        }
                    }
                    #[allow(clippy::collapsible_match)]
                    Ok(CrosstermEvent::Mouse(mouse)) => {
                        if tx_clone.send(TuiEvent::MouseInput(mouse)).await.is_err() {
                            break;
                        }
                    }
                    #[allow(clippy::collapsible_match)]
                    Ok(CrosstermEvent::Resize(w, h)) => {
                        if tx_clone.send(TuiEvent::Resize(w, h)).await.is_err() {
                            break;
                        }
                    }
                    _ => {}
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
        state.file_tree = filter_tui_test_files(files);
    }
    // 载入历史记录
    state.history_list = crate::history::storage::get_storage()
        .tail(100)
        .unwrap_or_default();
    state.history_list.reverse();

    // 默认加载首个文件或指定的 initial_file
    let mut initial_idx = 0;
    if let Some(init_path) = &initial_file {
        let matched = state.file_tree.iter().position(|p| {
            p == init_path
                || std::path::Path::new(p)
                    .canonicalize()
                    .map(|c| {
                        std::path::Path::new(init_path)
                            .canonicalize()
                            .map(|ic| c == ic)
                            .unwrap_or(false)
                    })
                    .unwrap_or(false)
        });
        if let Some(pos) = matched {
            initial_idx = pos;
        }
    }

    if !state.file_tree.is_empty() && initial_idx < state.file_tree.len() {
        let first_file = &state.file_tree[initial_idx];
        if let Ok(content) = std::fs::read_to_string(first_file) {
            state.editor_text = content;
            state.editor_file_path = Some(first_file.clone());
            if let Ok(metadata) = std::fs::metadata(first_file) {
                state.editor_file_mtime = metadata.modified().ok();
            }
            state.selected_file_index = initial_idx;
            state.files_state.selected_index = initial_idx;
            state.loaded_file_index = initial_idx;
            state.editor_scroll = 0;
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
                super::ui::render(frame, &mut state);
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
                                                    state.editor_text = content;
                                                    state.editor_file_path = Some(path.clone());
                                                    state.is_dirty = false;
                                                    state.loaded_file_index = idx;
                                                    state.selected_file_index = idx;
                                                    state.files_state.selected_index = idx;
                                                    state.editor_scroll = 0;
                                                    state.active_panel =
                                                        super::state::Panel::Editor;
                                                }
                                            }
                                        }
                                        super::state::PendingAction::SwitchHistory(idx) => {
                                            if idx < state.history_list.len() {
                                                state.history_state.selected_index = idx;
                                                let entry = &state.history_list[idx];
                                                let http_text =
                                                    super::state::format_request_snapshot_to_http(
                                                        &entry.request,
                                                    );
                                                state.editor_text = http_text;
                                                state.editor_file_path = None;
                                                state.is_dirty = false;
                                                load_history_response_to_state(&mut state);
                                                state.editor_scroll = 0;
                                                state.active_panel = super::state::Panel::Editor;
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
                        state.update(super::event::Action::Quit);
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
                                sync_preview_to_editor(&mut state);
                            }
                            crossterm::event::KeyCode::Right
                            | crossterm::event::KeyCode::Char('l') => {
                                state.active_sidebar_tab = super::state::SidebarTab::History;
                                sync_preview_to_editor(&mut state);
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
                                        if state.files_state.selected_index + 1
                                            < state.file_tree.len()
                                        {
                                            state.files_state.selected_index += 1;
                                            state.selected_file_index =
                                                state.files_state.selected_index;
                                            sync_preview_to_editor(&mut state);
                                        }
                                    }
                                    super::state::SidebarTab::History => {
                                        if state.history_state.selected_index + 1
                                            < state.history_list.len()
                                        {
                                            state.history_state.selected_index += 1;
                                            sync_preview_to_editor(&mut state);
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
                                            state.selected_file_index =
                                                state.files_state.selected_index;
                                            sync_preview_to_editor(&mut state);
                                        }
                                    }
                                    super::state::SidebarTab::History => {
                                        if state.history_state.selected_index > 0 {
                                            state.history_state.selected_index -= 1;
                                            sync_preview_to_editor(&mut state);
                                        }
                                    }
                                }
                            }
                            crossterm::event::KeyCode::Enter => match state.active_sidebar_tab {
                                super::state::SidebarTab::Files => {
                                    if !state.file_tree.is_empty() {
                                        state.active_panel = super::state::Panel::Editor;
                                    }
                                }
                                super::state::SidebarTab::History => {
                                    if !state.history_list.is_empty() {
                                        state.active_panel = super::state::Panel::Editor;
                                    }
                                }
                            },
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
                        let total_lines = state.editor_text.lines().count();
                        if is_run_key {
                            if !state.is_loading {
                                let parsed_req = if state.active_sidebar_tab
                                    == super::state::SidebarTab::History
                                {
                                    state.current_request.clone()
                                } else {
                                    match crate::parser::parse_content(&state.editor_text) {
                                        Ok(parsed) => parsed.requests.first().cloned(),
                                        Err(e) => {
                                            state.last_response =
                                                Some(crate::http::Response::error(format!(
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

                                    let (stream_tx, mut stream_rx) =
                                        tokio::sync::mpsc::unbounded_channel();

                                    let executor =
                                        crate::runner::TestExecutor::with_ephemeral_cookies()
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
                                                crate::runner::types::StreamEvent::SseChunk(
                                                    chunk,
                                                ) => {
                                                    let _ = s_tx
                                                        .send(TuiEvent::StreamChunk {
                                                            id: req_id,
                                                            chunk,
                                                            total_lines: 0,
                                                        })
                                                        .await;
                                                }
                                                crate::runner::types::StreamEvent::WsFrame {
                                                    is_send,
                                                    content,
                                                } => {
                                                    let _ = s_tx
                                                        .send(TuiEvent::WsFrame {
                                                            id: req_id,
                                                            is_send,
                                                            content,
                                                            total_lines: 0,
                                                        })
                                                        .await;
                                                }
                                                crate::runner::types::StreamEvent::InitLogPath(
                                                    path,
                                                ) => {
                                                    let _ = s_tx
                                                        .send(TuiEvent::InitLogPath {
                                                            id: req_id,
                                                            path,
                                                        })
                                                        .await;
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

                                        if let Some(ref resp) = test_res.response {
                                            let req_snapshot =
                                                crate::history::model::RequestSnapshot::from_parsed(
                                                    &req_snap_arg,
                                                );
                                            crate::history::recorder::record_history(
                                                req_snapshot,
                                                resp,
                                                source_clone,
                                            );
                                        }

                                        let captured_vars = var_context.variables().clone();

                                        let result = if test_res.success {
                                            if let Some(resp) = test_res.response {
                                                Ok(resp)
                                            } else {
                                                Err("Request succeeded but no response returned"
                                                    .to_string())
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
                                } else if state.last_response.is_none() {
                                    state.last_response = Some(crate::http::Response::error(
                                        "No request block found in editor.".to_string(),
                                    ));
                                    state.response_visual_lines.clear();
                                }
                            }
                        } else {
                            match key.code {
                                crossterm::event::KeyCode::Up
                                | crossterm::event::KeyCode::Char('k') => {
                                    state.editor_scroll = state.editor_scroll.saturating_sub(1);
                                }
                                crossterm::event::KeyCode::Down
                                | crossterm::event::KeyCode::Char('j') => {
                                    if state.editor_scroll + 1 < total_lines {
                                        state.editor_scroll += 1;
                                    }
                                }
                                crossterm::event::KeyCode::PageUp => {
                                    state.editor_scroll = state.editor_scroll.saturating_sub(10);
                                }
                                crossterm::event::KeyCode::PageDown => {
                                    state.editor_scroll = (state.editor_scroll + 10)
                                        .min(total_lines.saturating_sub(1));
                                }
                                _ => {}
                            }
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
                                let visible_height =
                                    state.terminal_height.saturating_sub(2) as usize;
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
                }
                TuiEvent::Resize(w, h) => {
                    state.update_layout(w, h);
                }
                TuiEvent::MouseInput(mouse_event) => {
                    if mouse_event.kind
                        == crossterm::event::MouseEventKind::Down(
                            crossterm::event::MouseButton::Left,
                        )
                    {
                        let x = mouse_event.column;
                        let y = mouse_event.row;
                        let w = state.terminal_width;
                        let h = state.terminal_height;

                        match state.layout_mode {
                            crate::tui::state::LayoutMode::Wide => {
                                let sidebar_w = (w as f32 * 0.25) as u16;
                                let editor_w = (w as f32 * 0.40) as u16;
                                if y == 0 && x < sidebar_w {
                                    state.active_panel = crate::tui::state::Panel::Files;
                                    if x < 11 {
                                        state.active_sidebar_tab =
                                            crate::tui::state::SidebarTab::Files;
                                    } else if (12..24).contains(&x) {
                                        state.active_sidebar_tab =
                                            crate::tui::state::SidebarTab::History;
                                    }
                                    sync_preview_to_editor(&mut state);
                                } else if y >= 1 && y < h - 1 && x < sidebar_w {
                                    state.active_panel = crate::tui::state::Panel::Files;
                                    let click_row = y.saturating_sub(2) as usize;
                                    match state.active_sidebar_tab {
                                        crate::tui::state::SidebarTab::Files => {
                                            let idx = state.files_state.scroll_offset + click_row;
                                            if idx < state.file_tree.len() {
                                                state.files_state.selected_index = idx;
                                                state.selected_file_index = idx;
                                                sync_preview_to_editor(&mut state);
                                            }
                                        }
                                        crate::tui::state::SidebarTab::History => {
                                            let idx = state.history_state.scroll_offset + click_row;
                                            if idx < state.history_list.len() {
                                                state.history_state.selected_index = idx;
                                                sync_preview_to_editor(&mut state);
                                            }
                                        }
                                    }
                                } else if x >= sidebar_w && x < sidebar_w + editor_w && y < h - 1 {
                                    state.active_panel = crate::tui::state::Panel::Editor;
                                } else if x >= sidebar_w + editor_w && x < w && y < h - 1 {
                                    state.active_panel = crate::tui::state::Panel::Response;
                                }
                            }
                            crate::tui::state::LayoutMode::Narrow => {
                                let sidebar_w = (w as f32 * 0.20) as u16;
                                let editor_w = (w as f32 * 0.40) as u16;
                                if y == 0 && x < sidebar_w {
                                    state.active_panel = crate::tui::state::Panel::Files;
                                    if x < 11 {
                                        state.active_sidebar_tab =
                                            crate::tui::state::SidebarTab::Files;
                                    } else if (12..24).contains(&x) {
                                        state.active_sidebar_tab =
                                            crate::tui::state::SidebarTab::History;
                                    }
                                    sync_preview_to_editor(&mut state);
                                } else if y >= 1 && y < h - 1 && x < sidebar_w {
                                    state.active_panel = crate::tui::state::Panel::Files;
                                    let click_row = y.saturating_sub(2) as usize;
                                    match state.active_sidebar_tab {
                                        crate::tui::state::SidebarTab::Files => {
                                            let idx = state.files_state.scroll_offset + click_row;
                                            if idx < state.file_tree.len() {
                                                state.files_state.selected_index = idx;
                                                state.selected_file_index = idx;
                                                sync_preview_to_editor(&mut state);
                                            }
                                        }
                                        crate::tui::state::SidebarTab::History => {
                                            let idx = state.history_state.scroll_offset + click_row;
                                            if idx < state.history_list.len() {
                                                state.history_state.selected_index = idx;
                                                sync_preview_to_editor(&mut state);
                                            }
                                        }
                                    }
                                } else if x >= sidebar_w && x < sidebar_w + editor_w && y < h - 1 {
                                    state.active_panel = crate::tui::state::Panel::Editor;
                                } else if x >= sidebar_w + editor_w && x < w && y < h - 1 {
                                    state.active_panel = crate::tui::state::Panel::Response;
                                }
                            }
                            crate::tui::state::LayoutMode::Stacked => {
                                if y <= 2 {
                                    let col_w = w / 3;
                                    if x < col_w {
                                        state.active_panel = crate::tui::state::Panel::Files;
                                    } else if x >= col_w && x < col_w * 2 {
                                        state.active_panel = crate::tui::state::Panel::Editor;
                                    } else {
                                        state.active_panel = crate::tui::state::Panel::Response;
                                    }
                                } else if y > 2
                                    && y < h - 1
                                    && state.active_panel == crate::tui::state::Panel::Files
                                {
                                    if y == 3 {
                                        if x < 11 {
                                            state.active_sidebar_tab =
                                                crate::tui::state::SidebarTab::Files;
                                        } else if (12..24).contains(&x) {
                                            state.active_sidebar_tab =
                                                crate::tui::state::SidebarTab::History;
                                        }
                                        sync_preview_to_editor(&mut state);
                                    } else if y >= 4 {
                                        let click_row = y.saturating_sub(5) as usize;
                                        match state.active_sidebar_tab {
                                            crate::tui::state::SidebarTab::Files => {
                                                let idx =
                                                    state.files_state.scroll_offset + click_row;
                                                if idx < state.file_tree.len() {
                                                    state.files_state.selected_index = idx;
                                                    state.selected_file_index = idx;
                                                    sync_preview_to_editor(&mut state);
                                                }
                                            }
                                            crate::tui::state::SidebarTab::History => {
                                                let idx =
                                                    state.history_state.scroll_offset + click_row;
                                                if idx < state.history_list.len() {
                                                    state.history_state.selected_index = idx;
                                                    sync_preview_to_editor(&mut state);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                TuiEvent::RequestFinished {
                    result,
                    captured_vars,
                    assertions,
                    ..
                } => {
                    state.handle_request_finished(*result, captured_vars, assertions);
                    state.response_visual_lines.clear(); // 清空缓存以强迫重新计算预折行
                    state.response_scroll = 0; // 请求结束重置滚动

                    // 从存储重新载入最新 100 条请求历史并倒序
                    state.history_list = crate::history::storage::get_storage()
                        .tail(100)
                        .unwrap_or_default();
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
                TuiEvent::Tick => {
                    state.check_and_reload_editor_file();
                }
                _ => {}
            }
        }

        if state.is_quitting {
            break;
        }
    }

    // 4. 清理并还原终端
    disable_raw_mode().map_err(crate::error::RupostError::IoError)?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )
    .map_err(crate::error::RupostError::IoError)?;
    terminal
        .show_cursor()
        .map_err(|e| crate::error::RupostError::IoError(std::io::Error::other(e.to_string())))?;

    Ok(())
}

fn load_history_response_to_state(state: &mut crate::tui::state::AppState) {
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

fn sync_preview_to_editor(state: &mut crate::tui::state::AppState) {
    if state.is_dirty {
        return;
    }
    match state.active_sidebar_tab {
        crate::tui::state::SidebarTab::Files => {
            if !state.file_tree.is_empty()
                && state.files_state.selected_index < state.file_tree.len()
            {
                let path = &state.file_tree[state.files_state.selected_index];
                if let Ok(content) = std::fs::read_to_string(path) {
                    state.editor_text = content;
                    state.editor_file_path = Some(path.clone());
                    if let Ok(metadata) = std::fs::metadata(path) {
                        state.editor_file_mtime = metadata.modified().ok();
                    } else {
                        state.editor_file_mtime = None;
                    }
                    state.is_dirty = false;
                    state.loaded_file_index = state.files_state.selected_index;
                    state.editor_scroll = 0; // 重置滚动
                }
            }
        }
        crate::tui::state::SidebarTab::History => {
            if !state.history_list.is_empty()
                && state.history_state.selected_index < state.history_list.len()
            {
                let entry = &state.history_list[state.history_state.selected_index];
                let http_text = crate::tui::state::format_request_snapshot_to_http(&entry.request);
                state.editor_text = http_text;
                state.editor_file_path = None;
                state.editor_file_mtime = None;
                state.is_dirty = false;
                load_history_response_to_state(state);
                state.editor_scroll = 0; // 重置滚动
            }
        }
    }
}

/// 对 TUI 中扫描出的所有候选文件路径进行精细化过滤
/// 1. 过滤文件名黑名单：README.md, SUMMARY.md, CHANGELOG.md, AGENTS.md, checkpoint.md, walkthrough.md 等
/// 2. 过滤目录黑名单：.git, .rupost, target, node_modules, .agents, .gemini 等
/// 3. 轻量内容启发式扫描：读取前缀 1024 字节，检查是否是包含有效 HTTP 请求的 http 文件或包含 http 代码块的 md 文件
pub fn filter_tui_test_files(paths: Vec<std::path::PathBuf>) -> Vec<String> {
    let mut result = Vec::new();

    // 黑名单目录列表
    let blacklisted_dirs = [
        ".git",
        ".rupost",
        "target",
        "node_modules",
        ".agents",
        ".gemini",
    ];

    // 黑名单文件名列表
    let blacklisted_files = [
        "README.md",
        "readme.md",
        "SUMMARY.md",
        "summary.md",
        "CHANGELOG.md",
        "changelog.md",
        "AGENTS.md",
        "agents.md",
        "checkpoint.md",
        "walkthrough.md",
    ];

    for path in paths {
        // 1. 第一层过滤：路径名及黑名单拦截（零 IO 读盘）
        // 检查是否包含黑名单目录
        let has_blacklisted_dir = blacklisted_dirs
            .iter()
            .any(|&dir| path.components().any(|c| c.as_os_str() == dir));
        if has_blacklisted_dir {
            continue;
        }

        // 检查文件名是否在黑名单中
        if let Some(file_name) = path.file_name() {
            let name_str = file_name.to_string_lossy();
            if blacklisted_files
                .iter()
                .any(|&f| name_str.eq_ignore_ascii_case(f))
            {
                continue;
            }
        }

        // 2. 第二层过滤：轻量级启发式读盘检视
        if let Ok(mut file) = std::fs::File::open(&path) {
            use std::io::Read;
            let mut buf = [0u8; 1024];
            if let Ok(bytes_read) = file.read(&mut buf) {
                let content = String::from_utf8_lossy(&buf[..bytes_read]);

                let is_http_extension = path
                    .extension()
                    .map(|ext| ext.to_string_lossy().eq_ignore_ascii_case("http"))
                    .unwrap_or(false);
                let is_md_extension = path
                    .extension()
                    .map(|ext| ext.to_string_lossy().eq_ignore_ascii_case("md"))
                    .unwrap_or(false);

                if is_http_extension {
                    let has_request = content.lines().any(|line| {
                        let trimmed = line.trim();
                        trimmed.starts_with("GET ")
                            || trimmed.starts_with("POST ")
                            || trimmed.starts_with("PUT ")
                            || trimmed.starts_with("DELETE ")
                            || trimmed.starts_with("PATCH ")
                            || trimmed.starts_with("HEAD ")
                            || trimmed.starts_with("OPTIONS ")
                            || trimmed.starts_with("WEBSOCKET ")
                            || trimmed.starts_with("WS ")
                    });
                    if !has_request {
                        continue;
                    }
                } else if is_md_extension {
                    if !content.contains("```http") && !content.contains("```rest") {
                        continue;
                    }
                } else {
                    continue;
                }
            } else {
                continue;
            }
        } else {
            continue;
        }

        result.push(path.to_string_lossy().to_string());
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_filter_tui_test_files_cases() {
        let dir = tempdir().unwrap();

        // 1. 创建合格的 .http 文件
        let http_ok = dir.path().join("ok.http");
        std::fs::write(&http_ok, "GET http://example.com").unwrap();

        // 2. 创建合格的 .md 文件
        let md_ok = dir.path().join("ok.md");
        std::fs::write(&md_ok, "# Doc\n```http\nPOST /login\n```").unwrap();

        // 3. 创建不合格的普通 README
        let readme = dir.path().join("README.md");
        std::fs::write(&readme, "# README\nThis is just a doc.").unwrap();

        // 4. 创建空 .http 文件
        let empty_http = dir.path().join("empty.http");
        std::fs::write(&empty_http, "   \n  ").unwrap();

        // 5. 创建非 UTF-8 二进制大文件
        let binary_http = dir.path().join("binary.http");
        let mut f = std::fs::File::create(&binary_http).unwrap();
        f.write_all(&[0u8, 159u8, 146u8, 150u8, 0xffu8, 0x00u8])
            .unwrap();

        // 6. 创建黑名单目录下的合格文件
        let node_modules_dir = dir.path().join("node_modules");
        std::fs::create_dir(&node_modules_dir).unwrap();
        let http_node = node_modules_dir.join("test.http");
        std::fs::write(&http_node, "GET http://localhost").unwrap();

        let paths = vec![
            http_ok.clone(),
            md_ok.clone(),
            readme,
            empty_http,
            binary_http,
            http_node,
        ];

        let filtered = filter_tui_test_files(paths);
        assert_eq!(filtered.len(), 2);

        let has_http = filtered.iter().any(|p| p.contains("ok.http"));
        let has_md = filtered.iter().any(|p| p.contains("ok.md"));
        assert!(has_http);
        assert!(has_md);
    }
}
