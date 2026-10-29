//! 编辑器组件按键处理与异步请求执行触发。

use super::nav;
use crate::tui::event::{Action, TuiEvent};
use crate::tui::state::{AppState, SidebarTab};
use crossterm::event::{KeyCode, KeyEvent};
use tokio::sync::mpsc;

/// 处理编辑器面板按键（包括触发运行测试请求）
/// 触发异步发包测试执行的主控逻辑 (共用于 Files 侧边栏 r/R 键与 Editor 区域 Ctrl+Enter)
pub async fn trigger_request_execution(state: &mut AppState, event_tx: &mpsc::Sender<TuiEvent>) {
    if state.is_loading {
        return;
    }
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
        state.update(Action::SendRequest(Box::new(req.clone())));

        let mut var_context = state.variables.clone();
        var_context.insert("__default_scheme", "http");

        let (stream_tx, mut stream_rx) = mpsc::unbounded_channel();
        let executor =
            crate::runner::TestExecutor::with_ephemeral_cookies().with_stream_sender(stream_tx);
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
                    warnings: test_res.warnings,
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

/// 处理编辑器面板按键（包括触发运行测试请求）
pub(super) async fn handle_editor_key(
    state: &mut AppState,
    key: KeyEvent,
    event_tx: &mpsc::Sender<TuiEvent>,
) {
    if state.show_help {
        return;
    }
    let total_lines = state.editor_text.lines().count();

    if nav::is_run_key(&key) {
        trigger_request_execution(state, event_tx).await;
    } else {
        let max_offset = nav::editor_max_offset(total_lines);
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                state.editor_scroll = nav::scroll_up(state.editor_scroll, 1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                state.editor_scroll = nav::scroll_down(state.editor_scroll, max_offset, 1);
            }
            KeyCode::PageUp => {
                state.editor_scroll = nav::scroll_up(state.editor_scroll, 10);
            }
            KeyCode::PageDown => {
                state.editor_scroll = nav::scroll_down(state.editor_scroll, max_offset, 10);
            }
            _ => {}
        }
    }
}
