//! TUI 关键动作与守卫分支直测（缺口 4 P1）。
//!
//! 契约：
//! 1. `ws_session::start_ws_session` 边界守卫覆盖：
//!    - `state.is_loading = true` 触发 "busy" 互斥；
//!    - 编辑器无请求块触发 "no request block" 拦截；
//!    - 编辑器非 @websocket 请求触发 "not a @websocket request" 拦截；
//!    - 会话已激活时重复触发 "already active" 互斥。
//! 2. `integrations::replay_latest_snapshot` 错误流覆盖：
//!    - 无快照文件时在状态栏直接浮出 "no snapshot files" 提示；
//!    - 快照文件损坏时通过事件通道回流 `TuiEvent::ReplayFailed`。
//! 3. 所有状态均通过 `TestBackend` 离屏渲染验证无 panic 且状态栏包含对应错误字样。
#![allow(clippy::collapsible_if, clippy::result_large_err)]

use futures_util::{SinkExt, StreamExt};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use rupost::tui::event::TuiEvent;
use rupost::tui::handlers::start_ws_session;
use rupost::tui::integrations::replay_latest_snapshot;
use rupost::tui::state::AppState;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::protocol::Message;

fn extract_buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
    let mut lines = Vec::new();
    for y in 0..buffer.area.height {
        let mut row = String::new();
        for x in 0..buffer.area.width {
            row.push_str(buffer[(x, y)].symbol());
        }
        lines.push(row.trim_end().to_string());
    }
    lines.join("\n")
}

fn assert_rendered_error(state: &mut AppState, expected_fragment: &str) {
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).expect("init test terminal");
    state.update_layout(120, 30);
    let draw_res = terminal.draw(|f| {
        rupost::tui::ui::render(f, state);
    });
    assert!(draw_res.is_ok(), "off-screen render must not panic");

    let screen_text = extract_buffer_text(terminal.backend().buffer());
    assert!(
        screen_text.contains(expected_fragment),
        "off-screen buffer must contain expected error fragment '{expected_fragment}', got:\n{screen_text}"
    );
}

async fn spawn_echo_ws_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await {
            if let Ok(mut ws) = tokio_tungstenite::accept_async(stream).await {
                while let Some(Ok(msg)) = ws.next().await {
                    match msg {
                        Message::Text(t) => {
                            let _ = ws.send(Message::Text(format!("echo: {t}"))).await;
                        }
                        Message::Close(_) => break,
                        _ => {}
                    }
                }
            }
        }
    });
    format!("ws://127.0.0.1:{port}")
}

#[tokio::test]
async fn test_start_ws_session_busy_guard() {
    let mut state = AppState::new();
    state.is_loading = true;
    let (tx, _rx) = mpsc::channel(16);

    start_ws_session(&mut state, &tx).await;

    assert!(state.ws_session.is_none());
    let err = state.last_error.as_deref().expect("last_error must be set");
    assert!(err.contains("busy"), "expected busy error, got: {err}");

    assert_rendered_error(&mut state, "busy");
}

#[tokio::test]
async fn test_start_ws_session_no_request_block_guard() {
    let mut state = AppState::new();
    state.editor_text = "# just comments\n# no actual request line\n".to_string();
    let (tx, _rx) = mpsc::channel(16);

    start_ws_session(&mut state, &tx).await;

    assert!(state.ws_session.is_none());
    let err = state.last_error.as_deref().expect("last_error must be set");
    assert!(
        err.contains("no request block"),
        "expected no request block error, got: {err}"
    );

    assert_rendered_error(&mut state, "no request block");
}

#[tokio::test]
async fn test_start_ws_session_non_websocket_guard() {
    let mut state = AppState::new();
    state.editor_text = "### Normal HTTP\nGET http://127.0.0.1:8080/api/users\n".to_string();
    let (tx, _rx) = mpsc::channel(16);

    start_ws_session(&mut state, &tx).await;

    assert!(state.ws_session.is_none());
    let err = state.last_error.as_deref().expect("last_error must be set");
    assert!(
        err.contains("not a @websocket request"),
        "expected not websocket error, got: {err}"
    );

    assert_rendered_error(&mut state, "not a @websocket request");
}

#[tokio::test]
async fn test_start_ws_session_already_active_guard() {
    let ws_url = spawn_echo_ws_server().await;
    let mut state = AppState::new();
    state.editor_text = format!("### Test WS\n@websocket\n@assert status == 101\n{ws_url}\n");
    let (tx, _rx) = mpsc::channel(16);

    // 第一次建连成功
    start_ws_session(&mut state, &tx).await;
    assert!(state.ws_session.is_some(), "session should be active");
    assert_eq!(state.last_error, None);

    // 第二次重复建连，触发 already active 守卫
    start_ws_session(&mut state, &tx).await;
    let err = state.last_error.as_deref().expect("last_error must be set");
    assert!(
        err.contains("already active"),
        "expected already active error, got: {err}"
    );

    assert_rendered_error(&mut state, "already active");
}

#[tokio::test]
async fn test_replay_latest_snapshot_no_files_guard() {
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let mut state = AppState::new();
    state.workspace_root = temp_dir.path().to_path_buf();
    let (tx, _rx) = mpsc::channel(16);

    replay_latest_snapshot(&mut state, &tx).await;

    let err = state.last_error.as_deref().expect("last_error must be set");
    assert!(
        err.contains("no snapshot files"),
        "expected no snapshot files error, got: {err}"
    );

    assert_rendered_error(&mut state, "no snapshot files");
}

#[tokio::test]
async fn test_replay_latest_snapshot_failed_stream_event() {
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let snapshots_dir = temp_dir.path().join("snapshots");
    std::fs::create_dir_all(&snapshots_dir).expect("create snapshots dir");

    // 写入一个损坏的 snapshot json 文件
    let snapshot_file = snapshots_dir.join("snapshot_20261110_000000.json");
    std::fs::write(&snapshot_file, "{ corrupted json without proper fields ")
        .expect("write corrupted snapshot");

    let mut state = AppState::new();
    state.workspace_root = temp_dir.path().to_path_buf();
    let (tx, mut rx) = mpsc::channel(16);

    replay_latest_snapshot(&mut state, &tx).await;

    // 前台应该没有直接报错，任务在后台异步执行
    assert_eq!(state.last_error, None);

    // 应该从事件流通道中接收到 ReplayFailed 事件
    let received = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("should receive event within timeout")
        .expect("channel should not be closed");

    match received {
        TuiEvent::ReplayFailed(err_msg) => {
            assert!(
                err_msg.contains("replay failed"),
                "expected replay failed message, got: {err_msg}"
            );
        }
        _other => panic!("expected ReplayFailed event"),
    }
}
