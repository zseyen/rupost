//! 编辑器组件按键处理与异步请求执行触发。

use super::nav;
use crate::tui::event::{Action, TuiEvent};
use crate::tui::state::{AppState, InputMode, SidebarTab};
use crossterm::event::{KeyCode, KeyEvent};
use tokio::sync::mpsc;

/// 快速授权注入（R3 决策表）：
/// - 未设置快速授权 → 原样
/// - 已设置且请求未显式声明 Authorization → 注入
/// - 已设置但请求显式声明了 Authorization → 显式优先，原样
pub fn inject_quick_auth(req: &mut crate::parser::ParsedRequest, auth: Option<String>) {
    let Some(value) = auth else {
        return;
    };
    let has_explicit = req
        .headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("authorization"));
    if !has_explicit {
        req.headers
            .push(("Authorization".to_string(), value.to_string()));
    }
}

/// 处理编辑器面板按键（包括触发运行测试请求）
/// 触发异步发包测试执行的主控逻辑 (共用于 Files 侧边栏 r/R 键与 Editor 区域 Ctrl+Enter)
pub async fn trigger_request_execution(state: &mut AppState, event_tx: &mpsc::Sender<TuiEvent>) {
    if state.is_loading {
        return;
    }
    // R7 互斥：交互会话活跃时禁止剧本执行（连接与变量上下文冲突）
    if state.ws_session.is_some() {
        state.last_error =
            Some("interactive WS session active — press D to close before running".to_string());
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

    if let Some(mut req) = parsed_req {
        // 快速授权注入（显式 Authorization 声明优先）
        inject_quick_auth(&mut req, state.quick_auth());
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

/// 处理编辑器面板按键（包括触发运行测试请求与 WS 交互会话）
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
            // R6：`W` 建立交互式 WS 会话（编辑器须为 @websocket 请求）
            KeyCode::Char('W') => start_ws_session(state, event_tx).await,
            // R6：`D` 关闭活跃会话（drop → SessionManager 退出 → 连接关闭）
            KeyCode::Char('D') => {
                if state.ws_session.take().is_some() {
                    state.last_error = None;
                }
            }
            // R6：`S` 打开发送输入框（会话活跃时）
            KeyCode::Char('S') => {
                if state.ws_session.is_some() {
                    state.input_mode = InputMode::WsSend(Box::new(
                        super::sidebar::new_input_textarea(state.palette()),
                    ));
                } else {
                    state.last_error =
                        Some("no active WS session — press W to open one".to_string());
                }
            }
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

/// 建立交互式 WS 会话（R6）：编辑器首个请求须为 @websocket；
/// 快速授权注入 + 变量解析后建连；帧经转发 task 回流既有渲染链路。
async fn start_ws_session(state: &mut AppState, event_tx: &mpsc::Sender<TuiEvent>) {
    if state.is_loading {
        state.last_error = Some("busy: request execution in flight".to_string());
        return;
    }
    if state.ws_session.is_some() {
        state.last_error = Some("WS session already active — press D to close first".to_string());
        return;
    }
    let parsed_req = match crate::parser::parse_content(&state.editor_text) {
        Ok(parsed) => parsed.requests.first().cloned(),
        Err(e) => {
            state.last_error = Some(format!("HTTP Parser Error: {e}"));
            return;
        }
    };
    let Some(mut req) = parsed_req else {
        state.last_error = Some("no request block found in editor".to_string());
        return;
    };
    if !req.metadata.websocket {
        state.last_error = Some(
            "editor is not a @websocket request — W opens interactive sessions only".to_string(),
        );
        return;
    }
    inject_quick_auth(&mut req, state.quick_auth());
    let mut var_context = state.variables.clone();
    var_context.insert("__default_scheme", "ws");

    match super::super::ws_interactive::start_session(&req, &var_context, event_tx.clone()).await {
        Ok(sess) => {
            state.last_error = None;
            state.ws_session = Some(sess);
        }
        Err(e) => state.last_error = Some(e),
    }
}

#[cfg(test)]
mod quick_auth_tests {
    use super::*;

    fn req_with_auth(auth: Option<&str>) -> crate::parser::ParsedRequest {
        let mut req = crate::parser::ParsedRequest {
            method: Some("GET".to_string()),
            url: "http://localhost".to_string(),
            headers: vec![],
            body: None,
            metadata: Default::default(),
            line_number: 1,
            base_path: None,
        };
        if let Some(v) = auth {
            req.headers
                .push(("Authorization".to_string(), v.to_string()));
        }
        req
    }

    /// R3 决策表：未设置 / 注入 / 显式优先
    #[test]
    fn test_inject_quick_auth_decision_table() {
        // 未设置快速授权 → 原样
        let mut req = req_with_auth(None);
        inject_quick_auth(&mut req, None);
        assert!(req.headers.is_empty());

        // 已设置 + 无显式声明 → 注入
        let mut req = req_with_auth(None);
        inject_quick_auth(&mut req, Some("Bearer t-1".to_string()));
        assert_eq!(req.headers.len(), 1);
        assert_eq!(req.headers[0].0, "Authorization");
        assert_eq!(req.headers[0].1, "Bearer t-1");

        // 已设置 + 显式声明 → 显式优先，不覆盖
        let mut req = req_with_auth(Some("Bearer explicit"));
        inject_quick_auth(&mut req, Some("Bearer quick".to_string()));
        assert_eq!(req.headers.len(), 1);
        assert_eq!(req.headers[0].1, "Bearer explicit");

        // 大小写不敏感的显式声明识别
        let mut req = req_with_auth(Some("Basic abc"));
        req.headers[0].0 = "authorization".to_string();
        inject_quick_auth(&mut req, Some("Bearer quick".to_string()));
        assert_eq!(req.headers.len(), 1);
        assert_eq!(req.headers[0].1, "Basic abc");
    }
}
