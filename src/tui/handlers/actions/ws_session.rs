//! WS 交互会话动作（R6/R7）：会话建立生命周期与流式域清空。

use super::execution::inject_quick_auth;
use crate::tui::event::TuiEvent;
use crate::tui::state::AppState;
use tokio::sync::mpsc;

/// 建立交互式 WS 会话（R6）：编辑器首个请求须为 @websocket；
/// 快速授权注入 + 变量解析后建连；帧经转发 task 回流既有渲染链路。
pub(crate) async fn start_ws_session(state: &mut AppState, event_tx: &mpsc::Sender<TuiEvent>) {
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

    match crate::tui::ws_interactive::start_session(&req, &var_context, event_tx.clone()).await {
        Ok(sess) => {
            state.last_error = None;
            state.ws_session = Some(sess);
            // 交互会话无 runner 日志文件参与渲染；上一轮运行的日志/缓存/内存帧
            // 若不清空，会话视图中会被当作伪帧渲染（张冠李戴的 stale 内容）
            state.log_file_path = None;
            state.total_log_lines = 0;
            state.viewport_cache.clear();
            state.cache_start_line = 0;
            state.cache_end_line = 0;
            state.sse_stream_body.clear();
            state.sse_visual_lines.clear();
            state.sse_last_processed_pos = 0;
            state.ws_frames.clear();
            state.ws_visual_lines.clear();
            state.warnings.clear();
            state.response_scroll = 0;
        }
        Err(e) => state.last_error = Some(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::handlers::editor::handle_editor_key;
    use crate::tui::handlers::handle_key;
    use crate::tui::state::InputMode as Im;
    use crossterm::event::{KeyCode, KeyEvent};
    use futures_util::{SinkExt, StreamExt};
    use std::time::Duration;
    use tokio::net::TcpListener;

    // ---- 会话编排守护（自 editor.rs ws_orchestration_tests 迁入）----

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), crossterm::event::KeyModifiers::NONE)
    }

    fn ws_editor_state(url: &str) -> AppState {
        let mut state = AppState::new();
        state.editor_text = format!("### WS\n@websocket\n@assert status == 101\n{url}\n");
        state
    }

    fn plain_editor_state() -> AppState {
        let mut state = AppState::new();
        state.editor_text = "### Plain\nGET http://localhost\n".to_string();
        state
    }

    /// 本地 WS echo 服务器（accept + 原样回显 Text 帧）
    async fn spawn_echo_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await
                && let Ok(mut ws) = tokio_tungstenite::accept_async(stream).await
            {
                while let Some(Ok(msg)) = ws.next().await {
                    match msg {
                        tokio_tungstenite::tungstenite::protocol::Message::Text(t) => {
                            let _ = ws
                                .send(tokio_tungstenite::tungstenite::protocol::Message::Text(
                                    format!("echo: {t}"),
                                ))
                                .await;
                        }
                        _ => break,
                    }
                }
            }
        });
        format!("ws://127.0.0.1:{port}")
    }

    /// R6 守护：W 对非 @websocket 编辑器浮出可读错误、不建会话
    #[tokio::test]
    async fn test_w_key_rejects_non_websocket_editor() {
        let mut state = plain_editor_state();
        let (tx, _rx) = mpsc::channel(1);
        handle_editor_key(&mut state, key('W'), &tx).await;
        assert!(state.ws_session.is_none());
        let err = state.last_error.expect("readable error required");
        assert!(
            err.contains("@websocket"),
            "error must mention @websocket: {err}"
        );
    }

    /// R6 守护：S 无会话浮错不开模态；D 对空会话为无害操作
    #[tokio::test]
    async fn test_s_without_session_and_d_on_empty() {
        let mut state = plain_editor_state();
        let (tx, _rx) = mpsc::channel(1);
        handle_editor_key(&mut state, key('S'), &tx).await;
        assert!(state.ws_session.is_none());
        assert!(
            state
                .last_error
                .as_deref()
                .unwrap()
                .contains("no active WS session")
        );
        assert!(matches!(state.input_mode, Im::None));

        let mut state = plain_editor_state();
        handle_editor_key(&mut state, key('D'), &tx).await;
        assert!(state.ws_session.is_none(), "D on empty session is a no-op");
    }

    /// R6/R7 守护：W 建会话成功 → 重复 W 拒绝 → 剧本执行被互斥拒绝 →
    /// S 发送回显 is_send 记录 → D 关闭
    #[tokio::test]
    async fn test_ws_session_full_key_lifecycle() {
        let url = spawn_echo_server().await;
        let mut state = ws_editor_state(&url);
        let (tx, _rx) = mpsc::channel(64);

        // 1. W 建会话
        handle_editor_key(&mut state, key('W'), &tx).await;
        assert!(state.ws_session.is_some(), "session must be active");
        assert_eq!(state.last_error, None);

        // 2. 重复 W 被拒
        handle_editor_key(&mut state, key('W'), &tx).await;
        assert!(
            state
                .last_error
                .as_deref()
                .unwrap()
                .contains("already active"),
            "duplicate W must surface: {:?}",
            state.last_error
        );

        // 3. 剧本执行互斥（R7）
        crate::tui::handlers::actions::execution::trigger_request_execution(&mut state, &tx).await;
        assert!(
            state
                .last_error
                .as_deref()
                .unwrap()
                .contains("interactive WS session active"),
            "run must be rejected while session active: {:?}",
            state.last_error
        );

        // 4. S 开框 → 输入 → Enter 提交（pending）→ handle_key 派发回显
        state.active_panel = crate::tui::state::Panel::Editor;
        handle_key(&mut state, key('S'), &tx).await;
        assert!(matches!(state.input_mode, Im::WsSend(_)));
        for c in "ping-1".chars() {
            handle_key(&mut state, key(c), &tx).await;
        }
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, crossterm::event::KeyModifiers::NONE),
            &tx,
        )
        .await;
        assert!(matches!(state.input_mode, Im::None));
        assert!(state.pending_ws_send.is_none(), "pending consumed");

        // 出站回显帧（is_send=true）最终出现在 ws_frames
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline
            && !state
                .ws_frames
                .iter()
                .any(|f| f.is_send && f.content == "ping-1")
        {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(
            state
                .ws_frames
                .iter()
                .any(|f| f.is_send && f.content == "ping-1"),
            "sent frame must be echoed into ws_frames"
        );

        // 5. D 关闭
        handle_editor_key(&mut state, key('D'), &tx).await;
        assert!(state.ws_session.is_none());
        // 关闭后执行互斥解除：真实调用触发器验证守卫（同步，在解析/执行之前）
        // 不再因会话浮错；spawn 的执行对已关服务器无状态副作用（测试进程已静音历史落盘）
        tokio::time::sleep(Duration::from_millis(50)).await;
        state.last_error = None;
        crate::tui::handlers::actions::execution::trigger_request_execution(&mut state, &tx).await;
        assert!(
            !state
                .last_error
                .as_deref()
                .is_some_and(|e| e.contains("interactive WS session")),
            "guard must not fire after session close: {:?}",
            state.last_error
        );
    }

    /// R7 守护：会话缺失时 WsSend 模态 Enter 派发浮错
    /// （派发门在模态分支内：handle_modal_key 产生 pending 后同调用内消费）
    #[tokio::test]
    async fn test_orphan_pending_send_surfaces_error() {
        use ratatui_textarea::TextArea;
        let mut state = plain_editor_state();
        let (tx, _rx) = mpsc::channel(1);
        state.input_mode = Im::WsSend(Box::new(TextArea::default()));
        state.active_panel = crate::tui::state::Panel::Editor;

        // 输入文本（空文本 Enter 被忽略是既有守卫）
        for c in "orphan".chars() {
            handle_key(&mut state, key(c), &tx).await;
        }
        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, crossterm::event::KeyModifiers::NONE),
            &tx,
        )
        .await;
        assert!(matches!(state.input_mode, Im::None));
        assert!(state.pending_ws_send.is_none(), "pending consumed");
        assert!(
            state
                .last_error
                .as_deref()
                .unwrap()
                .contains("WS session closed"),
            "orphan send must surface: {:?}",
            state.last_error
        );
    }

    /// P0-2 守护：W 建会话必须清空上一轮运行的流式域（日志路径/行数/视口缓存/
    /// 内存帧/SSE 正文/告警），否则会话视图渲染 stale 伪帧、搜索空间错位
    #[tokio::test]
    async fn test_w_session_clears_stale_stream_state() {
        let url = spawn_echo_server().await;
        let mut state = ws_editor_state(&url);
        // 伪造上一轮运行残留
        state.log_file_path = Some(std::path::PathBuf::from("/tmp/stale-run.log"));
        state.total_log_lines = 42;
        state
            .viewport_cache
            .push_back(crate::tui::state::WsFrameRecord {
                is_send: false,
                timestamp: "11:00:00".to_string(),
                content: "stale-frame".to_string(),
            });
        state.cache_start_line = 5;
        state.cache_end_line = 40;
        state.sse_stream_body = "data: stale\n".to_string();
        state.ws_frames.push(crate::tui::state::WsFrameRecord {
            is_send: true,
            timestamp: "11:00:01".to_string(),
            content: "stale-send".to_string(),
        });
        state.warnings = vec!["stale warning".to_string()];

        let (tx, _rx) = mpsc::channel(64);
        handle_editor_key(&mut state, key('W'), &tx).await;
        assert!(state.ws_session.is_some(), "session must be established");

        assert!(state.log_file_path.is_none());
        assert_eq!(state.total_log_lines, 0);
        assert!(state.viewport_cache.is_empty());
        assert_eq!(state.cache_start_line, 0);
        assert_eq!(state.cache_end_line, 0);
        assert!(state.sse_stream_body.is_empty());
        assert!(state.ws_frames.is_empty());
        assert!(state.warnings.is_empty());

        // 会话期搜索空间必须是内存视觉行（与渲染分支一致），而非日志文件
        use crate::tui::state::search::SearchSpace;
        assert!(matches!(state.search_space(), SearchSpace::VisualLines(_)));
    }

    // ---- W 键握手接线守护（自 editor.rs wiring_tests 迁入）----

    /// 本地 WS echo 服务器（accept_hdr_async 捕获握手 header）
    #[allow(clippy::result_large_err)] // accept_hdr_async 的 Err 类型由 API 固定
    async fn spawn_ws_capture_server() -> (String, tokio::sync::mpsc::Receiver<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        tokio::spawn(async move {
            let tx = tx;
            if let Ok((stream, _)) = listener.accept().await {
                let captured: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
                    std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
                let capture = captured.clone();
                let callback =
                    |req: &tokio_tungstenite::tungstenite::http::Request<()>,
                     resp: tokio_tungstenite::tungstenite::http::Response<()>| {
                        for (k, v) in req.headers() {
                            capture.lock().unwrap().push(format!(
                                "{}: {}",
                                k.as_str(),
                                v.to_str().unwrap_or("")
                            ));
                        }
                        Ok(resp)
                    };
                if let Ok(mut ws) = tokio_tungstenite::accept_hdr_async(stream, callback).await {
                    let headers = captured.lock().unwrap().clone();
                    let _ = tx.send(headers).await;
                    while let Some(Ok(msg)) = ws.next().await {
                        match msg {
                            tokio_tungstenite::tungstenite::protocol::Message::Text(t) => {
                                let _ = ws
                                    .send(tokio_tungstenite::tungstenite::protocol::Message::Text(
                                        format!("echo: {t}"),
                                    ))
                                    .await;
                            }
                            _ => break,
                        }
                    }
                }
            }
        });
        (format!("http://127.0.0.1:{port}"), rx)
    }

    /// 接线守护（R6+R3）：W 键建连时快速授权经 start_ws_session 注入握手。
    /// 删掉 start_ws_session 内的 inject_quick_auth 调用 → 本测试红。
    #[tokio::test]
    #[allow(clippy::result_large_err)] // accept_hdr_async 的 Err 类型由 API 固定
    async fn test_w_key_carries_quick_auth_into_handshake() {
        let (url, mut rx) = spawn_ws_capture_server().await;
        let mut state = AppState::new();
        state.editor_text = format!("### WS\n@websocket\n{url}\n");
        state.set_quick_auth("Bearer ws-quick");

        let (tx, _rx) = mpsc::channel(8);
        handle_editor_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('W'), crossterm::event::KeyModifiers::NONE),
            &tx,
        )
        .await;
        assert!(state.ws_session.is_some(), "session must open");

        let headers = tokio::time::timeout(Duration::from_secs(3), rx.recv())
            .await
            .unwrap()
            .expect("handshake captured");
        assert!(
            headers
                .iter()
                .any(|h| h.eq_ignore_ascii_case("authorization: Bearer ws-quick")),
            "quick auth must reach WS handshake, got {headers:?}"
        );
    }

    /// 接线守护（R1+R6）：编辑器中的 {{变量}} 在 WS 会话建立时解析
    /// （state.variables 由启动加载链填充；删掉 build_initial_context
    /// 接线或 start_session 的 resolve → 本测试红）。
    #[tokio::test]
    async fn test_w_key_resolves_variables_into_handshake() {
        let (url, mut rx) = spawn_ws_capture_server().await;
        let mut state = AppState::new();
        state.editor_text = format!("### WS\n@websocket\n{url}\nX-Api: {{{{tok}}}}\n");
        state.variables.insert("tok", "resolved-v1");

        let (tx, _rx) = mpsc::channel(8);
        handle_editor_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('W'), crossterm::event::KeyModifiers::NONE),
            &tx,
        )
        .await;
        assert!(state.ws_session.is_some(), "session must open");

        let headers = tokio::time::timeout(Duration::from_secs(3), rx.recv())
            .await
            .unwrap()
            .expect("handshake captured");
        assert!(
            headers.iter().any(|h| h == "x-api: resolved-v1"),
            "variable must be resolved in WS handshake, got {headers:?}"
        );
    }
}
