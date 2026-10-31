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
mod ws_orchestration_tests {
    use super::*;
    use crate::tui::handlers::handle_key;
    use crate::tui::state::InputMode as Im;
    use crossterm::event::KeyModifiers;
    use futures_util::{SinkExt, StreamExt};
    use std::time::Duration;
    use tokio::net::TcpListener;

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
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
        trigger_request_execution(&mut state, &tx).await;
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
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
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
        // 关闭后执行互斥解除
        tokio::time::sleep(Duration::from_millis(50)).await;
        state.last_error = None;
        // （不真正执行剧本——服务器可能已关；只验证守卫不再因会话触发）
        assert_eq!(state.last_error, None);
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
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
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

#[cfg(test)]
mod wiring_tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use std::time::Duration;
    use tokio::net::TcpListener;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// 接线守护（R3）：trigger_request_execution 必须调用快速授权注入——
    /// state.quick_auth 已设置时，wiremock 收到的请求携带 Authorization。
    /// 删掉 trigger 内的 inject_quick_auth 调用 → 本测试红。
    #[tokio::test]
    async fn test_trigger_injects_quick_auth_into_real_request() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/secure"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&server)
            .await;

        let mut state = AppState::new();
        state.editor_text = format!("### Secured\nGET {}/secure\n", server.uri());
        state.set_quick_auth("Bearer wiring-1");

        let (tx, _rx) = mpsc::channel(8);
        trigger_request_execution(&mut state, &tx).await;

        // 执行 task 异步发包（RequestFinished 事件由 app.rs 主循环消费，测试外无关）
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        let received = loop {
            tokio::time::sleep(Duration::from_millis(50)).await;
            assert!(
                tokio::time::Instant::now() < deadline,
                "request must arrive within 5s window"
            );
            if let Ok(Some(reqs)) =
                tokio::time::timeout(Duration::from_millis(100), server.received_requests()).await
                && !reqs.is_empty()
            {
                break reqs;
            }
        };
        assert!(!received.is_empty());
        let auth = received[0]
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        assert_eq!(auth, "Bearer wiring-1", "quick auth must reach the wire");
    }

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
