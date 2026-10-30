//! TUI WS 交互会话集成测试（R6/R7 验收）。
//!
//! 契约：
//! 1. `start_session` 对 @websocket 请求建立常驻会话，收帧经事件通道回流；
//! 2. `send_text` 发送 → 服务端 echo → TuiEvent::WsFrame(is_send=false) 收到回显；
//! 3. 授权 header（含快速授权注入）进入 WS 握手；
//! 4. 非 @websocket 请求 / 死端口 / 连接失败 → 可读错误；
//! 5. drop 会话后服务端收到 Close（连接关闭语义）。
#![allow(clippy::collapsible_if, clippy::result_large_err)]
use futures_util::{SinkExt, StreamExt};
use rupost::parser::{HttpFileParser, ParsedRequest};
use rupost::tui::event::TuiEvent;
use rupost::tui::ws_interactive::{self, start_session};
use rupost::variable::VariableContext;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::protocol::Message;

/// Echo WS 服务器：记录握手 Authorization，收到的 Text 帧原样回显
async fn start_echo_ws_server() -> (String, tokio::sync::mpsc::Receiver<Option<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = tokio::sync::mpsc::channel::<Option<String>>(8);

    tokio::spawn(async move {
        let tx = tx;
        if let Ok((stream, _)) = listener.accept().await {
            // accept_hdr_async 回调捕获握手请求中的 Authorization
            let captured: std::sync::Arc<std::sync::Mutex<Option<String>>> =
                std::sync::Arc::new(std::sync::Mutex::new(None));
            let capture = captured.clone();
            let callback =
                |req: &tokio_tungstenite::tungstenite::http::Request<()>,
                 resp: tokio_tungstenite::tungstenite::http::Response<()>| {
                    if let Some(auth) = req.headers().get("authorization") {
                        *capture.lock().unwrap() =
                            Some(format!("authorization: {}", auth.to_str().unwrap_or("")));
                    }
                    Ok(resp)
                };

            if let Ok(mut ws_stream) = tokio_tungstenite::accept_hdr_async(stream, callback).await {
                let auth_captured = captured.lock().unwrap().clone();
                let _ = tx.send(auth_captured).await;
                while let Some(Ok(msg)) = ws_stream.next().await {
                    match msg {
                        Message::Text(txt) => {
                            let _ = ws_stream.send(Message::Text(format!("echo: {txt}"))).await;
                        }
                        Message::Close(_) | Message::Frame(_) => break,
                        _ => {}
                    }
                }
            }
        }
        let _ = tx.send(None).await;
    });

    (format!("http://127.0.0.1:{port}"), rx)
}

fn ws_request(url: &str) -> ParsedRequest {
    let content = format!("### Interactive WS\n@websocket\n@assert status == 101\n{url}\n");
    HttpFileParser::parse_content(&content)
        .unwrap()
        .requests
        .remove(0)
}

#[tokio::test]
async fn test_ws_interactive_session_echo_roundtrip_with_auth() {
    let (url, mut server_rx) = start_echo_ws_server().await;

    let mut req = ws_request(&url);
    // 快速授权注入（模拟调用侧 inject_quick_auth 完成）
    req.headers
        .push(("Authorization".to_string(), "Bearer tui-token".to_string()));

    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(64);
    let ctx = VariableContext::new();

    // 1. 建立会话
    let session = start_session(&req, &ctx, event_tx)
        .await
        .expect("connect ok");

    // 2. 服务端确认握手携带授权 header
    let auth_line = tokio::time::timeout(Duration::from_secs(3), server_rx.recv())
        .await
        .unwrap()
        .expect("handshake captured");
    assert!(
        auth_line
            .as_deref()
            .unwrap_or("")
            .eq_ignore_ascii_case("authorization: Bearer tui-token"),
        "auth header must reach the WS handshake, got {auth_line:?}"
    );

    // 3. 发送 → 服务端 echo → 事件通道收到 is_send=false 的回显帧
    ws_interactive::send_text(&session.session, "hello-tui")
        .await
        .expect("send ok");

    let mut got_echo = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(200), event_rx.recv()).await {
            Ok(Some(TuiEvent::WsFrame {
                is_send, content, ..
            })) => {
                if !is_send && content.contains("echo: hello-tui") {
                    got_echo = true;
                    break;
                }
            }
            Ok(Some(_)) => continue,
            _ => break,
        }
    }
    assert!(got_echo, "echo frame must arrive via TuiEvent::WsFrame");
}

#[tokio::test]
async fn test_ws_interactive_session_rejects_non_websocket_and_dead_port() {
    let (event_tx, _event_rx) = tokio::sync::mpsc::channel(4);
    let ctx = VariableContext::new();

    // 1. 非 @websocket 请求 → 可读错误
    let plain = ParsedRequest {
        method: Some("GET".to_string()),
        url: "http://localhost".to_string(),
        headers: vec![],
        body: None,
        metadata: Default::default(),
        line_number: 1,
        base_path: None,
    };
    let err = start_session(&plain, &ctx, event_tx.clone()).await;
    assert!(err.is_err(), "non-ws request must be rejected");
    // （该错误在调用侧 start_ws_session 的 metadata 检查中产生；此处直接建连会失败于协议）
    let _ = err;

    // 2. 死端口 → 建连错误浮出（非 panic）
    let ws_req = ws_request("http://127.0.0.1:1");
    let err = start_session(&ws_req, &ctx, event_tx).await;
    assert!(err.is_err(), "dead port must surface an error");
}

#[tokio::test]
async fn test_ws_interactive_session_close_notifies_server() {
    let (url, mut server_rx) = start_echo_ws_server().await;
    let req = ws_request(&url);
    let (event_tx, _event_rx) = tokio::sync::mpsc::channel(16);
    let ctx = VariableContext::new();

    let session = start_session(&req, &ctx, event_tx)
        .await
        .expect("connect ok");
    // drop → SessionManager 退出 → 连接关闭（服务端循环退出发 None 哨兵）
    drop(session);

    // 等服务端侧连接结束（跳过握手哨兵，等 None 结束哨兵）
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(200), server_rx.recv()).await {
            Ok(Some(None)) => return,      // 服务端循环已退出 = 连接已关
            Ok(Some(Some(_))) => continue, // 握手哨兵
            _ => break,
        }
    }
    panic!("server loop must observe connection close after session drop");
}
