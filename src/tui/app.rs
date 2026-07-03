use crate::Result;
use std::time::Duration;
use tokio::sync::mpsc;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    event::{self, Event as CrosstermEvent},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use super::event::TuiEvent;
use super::state::AppState;

/// 启动并运行 TUI 主事件循环
pub fn run() -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| crate::error::RupostError::IoError(e))?;
        
    rt.block_on(async {
        run_async().await
    })
}

async fn run_async() -> Result<()> {
    // 1. 初始化终端
    enable_raw_mode().map_err(|e| crate::error::RupostError::IoError(e))?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(|e| crate::error::RupostError::IoError(e))?;
    
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| crate::error::RupostError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;
    
    // 2. 建立 MPSC 事件通道
    let (event_tx, mut event_rx) = mpsc::channel(100);
    
    // 派发 crossterm 事件捕获 Task
    let tx_clone = event_tx.clone();
    tokio::spawn(async move {
        loop {
            // 阻断式轮询是否有 crossterm 事件
            if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                if let Ok(CrosstermEvent::Key(key)) = event::read() {
                    if tx_clone.send(TuiEvent::Input(key)).await.is_err() {
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
        state.file_tree = files.iter().map(|p| p.to_string_lossy().to_string()).collect();
    }
    
    let mut textarea = tui_textarea::TextArea::default();
    textarea.set_placeholder_text("Press Tab to focus and type URL\nOr select a file on the left panel.");
    
    // 默认加载首个文件
    if !state.file_tree.is_empty() {
        let first_file = &state.file_tree[0];
        if let Ok(content) = std::fs::read_to_string(first_file) {
            state.editor_text = content.clone();
            state.editor_file_path = Some(first_file.clone());
            textarea = tui_textarea::TextArea::new(content.lines().map(String::from).collect());
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
        terminal.draw(|frame| {
            super::ui::render(frame, &mut state, &mut textarea);
        }).map_err(|e| crate::error::RupostError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;
        
        // B. 接收事件
        if let Some(event) = event_rx.recv().await {
            match event {
                TuiEvent::Input(key) => {
                    // 未保存强确认弹窗前置网关
                    if state.show_unsaved_confirm {
                        match key.code {
                            crossterm::event::KeyCode::Char('y') | crossterm::event::KeyCode::Char('Y') => {
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
                                                    textarea = tui_textarea::TextArea::new(content.lines().map(String::from).collect());
                                                    state.is_dirty = false;
                                                    state.loaded_file_index = idx;
                                                    state.selected_file_index = idx;
                                                }
                                            }
                                        }
                                    }
                                }
                                state.show_unsaved_confirm = false;
                                state.pending_action = None;
                            }
                            crossterm::event::KeyCode::Char('n') | crossterm::event::KeyCode::Char('N') | crossterm::event::KeyCode::Esc => {
                                state.selected_file_index = state.loaded_file_index;
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

                    // Files 面板操作
                    if state.active_panel == super::state::Panel::Files && !state.show_help {
                        match key.code {
                            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j') => {
                                if state.selected_file_index + 1 < state.file_tree.len() {
                                    state.selected_file_index += 1;
                                }
                            }
                            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k') => {
                                if state.selected_file_index > 0 {
                                    state.selected_file_index -= 1;
                                }
                            }
                            crossterm::event::KeyCode::Enter => {
                                if !state.file_tree.is_empty() {
                                    let path = &state.file_tree[state.selected_file_index];
                                    if state.is_dirty {
                                        if state.selected_file_index != state.loaded_file_index {
                                            state.show_unsaved_confirm = true;
                                            state.pending_action = Some(super::state::PendingAction::SwitchFile(state.selected_file_index));
                                        }
                                    } else {
                                        if let Ok(content) = std::fs::read_to_string(path) {
                                            state.editor_text = content.clone();
                                            state.editor_file_path = Some(path.clone());
                                            textarea = tui_textarea::TextArea::new(content.lines().map(String::from).collect());
                                            state.is_dirty = false;
                                            state.loaded_file_index = state.selected_file_index;
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
                        let is_run_key = (key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) && key.code == crossterm::event::KeyCode::Char('r'))
                            || (key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) && key.code == crossterm::event::KeyCode::Enter);

                        if is_run_key {
                            if !state.is_loading {
                                let content = textarea.lines().join("\n");
                                if let Ok(parsed_file) = crate::parser::parse_content(&content) {
                                    if !parsed_file.requests.is_empty() {
                                        let req = parsed_file.requests[0].clone();
                                        state.is_loading = true;

                                        let mut var_context = state.variables.clone();
                                        var_context.insert("__default_scheme", "http");

                                        let executor = crate::runner::TestExecutor::with_ephemeral_cookies();
                                        let source = state.editor_file_path.clone();
                                        let tx_clone = event_tx.clone();
                                        let req_id = uuid::Uuid::new_v4();

                                        let _ = tx_clone.send(TuiEvent::RequestStarted(req_id)).await;

                                        tokio::spawn(async move {
                                            let test_res = executor.execute_one(req, 1, &mut var_context, source).await;
                                            let captured_vars = var_context.variables().clone();

                                            let result = if test_res.success {
                                                if let Some(resp) = test_res.response {
                                                    Ok(resp)
                                                } else {
                                                    Err("Request succeeded but no response returned".to_string())
                                                }
                                            } else {
                                                Err(test_res.error.unwrap_or_else(|| "Unknown execution error".to_string()))
                                            };

                                            let _ = tx_clone.send(TuiEvent::RequestFinished {
                                                id: req_id,
                                                result,
                                                captured_vars,
                                                assertions: test_res.assertions,
                                            }).await;
                                        });
                                    }
                                }
                            }
                        } else if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) && key.code == crossterm::event::KeyCode::Char('s') {
                            // Ctrl+S 保存
                            if let Some(ref path) = state.editor_file_path {
                                let text = textarea.lines().join("\n");
                                if std::fs::write(path, text).is_ok() {
                                    state.is_dirty = false;
                                }
                            }
                        } else if key.code != crossterm::event::KeyCode::Tab && key.code != crossterm::event::KeyCode::Char('?') {
                            // 其余非全局功能键则派发给 textarea
                            textarea.input(key);
                            state.is_dirty = true;
                            state.editor_text = textarea.lines().join("\n");
                        }
                    }
                }
                TuiEvent::Resize(w, h) => {
                    state.update_layout(w, h);
                }
                TuiEvent::RequestFinished { result, captured_vars, assertions, .. } => {
                    state.handle_request_finished(result, captured_vars, assertions);
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
    disable_raw_mode().map_err(|e| crate::error::RupostError::IoError(e))?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen).map_err(|e| crate::error::RupostError::IoError(e))?;
    terminal.show_cursor().map_err(|e| crate::error::RupostError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;
    
    Ok(())
}
