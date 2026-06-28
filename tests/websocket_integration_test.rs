use rupost::parser::HttpFileParser;
use rupost::runner::TestExecutor;
use rupost::variable::VariableContext;
use std::time::Duration;
use tokio::net::TcpListener;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::protocol::Message;

// 启动一个本地的 Mock WebSocket 服务器
async fn start_ws_mock_server() -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let ws_url = format!("ws://127.0.0.1:{}", port);

    let handle = tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await {
            if let Ok(mut ws_stream) = tokio_tungstenite::accept_async(stream).await {
                let mut heartbeat_timer = tokio::time::interval(Duration::from_millis(150));
                
                loop {
                    tokio::select! {
                        // 模拟高频业务心跳广播帧，用于验证客户端的软匹配与心跳防雪崩机制
                        _ = heartbeat_timer.tick() => {
                            let heartbeat_msg = Message::Text(r#"{"type":"heartbeat","data":"pulse"}"#.to_string());
                            if ws_stream.send(heartbeat_msg).await.is_err() {
                                break;
                            }
                        }
                        maybe_msg = ws_stream.next() => {
                            match maybe_msg {
                                Some(Ok(Message::Text(txt))) => {
                                    if txt.contains("subscribe_bin") {
                                        // 延迟发送预期的 MsgPack 订阅通知帧
                                        tokio::time::sleep(Duration::from_millis(200)).await;
                                        
                                        #[derive(serde::Serialize)]
                                        struct TickerBin {
                                            event: String,
                                            symbol: String,
                                            price: u64,
                                        }
                                        let ticker = TickerBin {
                                            event: "ticker".to_string(),
                                            symbol: "ETH".to_string(),
                                            price: 3200,
                                        };
                                        let bin = rmp_serde::to_vec_named(&ticker).unwrap();
                                        let ticker_msg = Message::Binary(bin);
                                        let _ = ws_stream.send(ticker_msg).await;
                                    } else if txt.contains("subscribe") || txt.contains("log_price") {
                                        // 延迟发送预期的订阅通知帧
                                        tokio::time::sleep(Duration::from_millis(200)).await;
                                        let ticker_msg = Message::Text(r#"{"event":"ticker","symbol":"BTC","price":62500}"#.to_string());
                                        let _ = ws_stream.send(ticker_msg).await;
                                    }
                                }
                                Some(Ok(Message::Close(_))) | None => {
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    });

    (ws_url, handle)
}

#[tokio::test]
async fn test_websocket_e2e_soft_matching_and_capture() {
    let (ws_url, _server_handle) = start_ws_mock_server().await;

    // 创建临时 HTTP 内容，带有 @websocket、断言以及变量捕获
    let temp_dir = tempfile::TempDir::new().unwrap();
    let http_file = temp_dir.path().join("ws_e2e_test.http");

    let content = format!(
        r#"
### WebSocket E2E soft matching and capture test
@websocket
@assert body.price > 60000
@assert body.symbol == "BTC"
@capture btc_price from body.price
GET {}

SEND {{ "action": "subscribe" }}
EXPECT {{"event": "ticker"}}
@timeout = 3000
CLOSE
"#,
        ws_url
    );
    std::fs::write(&http_file, content).unwrap();

    let parsed = HttpFileParser::parse_file(&http_file).unwrap();
    let executor = TestExecutor::new();
    let mut context = VariableContext::new();

    let results = executor.execute_all(parsed, &mut context).await.unwrap();
    assert_eq!(results.len(), 1);

    let res = &results[0];
    assert!(
        res.success,
        "WebSocket TestResult should be success. Error: {:?}",
        res.error
    );

    // 验证所有的断言均已通过
    assert!(!res.assertions.is_empty(), "Assertions list cannot be empty");
    for assert in &res.assertions {
        assert!(assert.passed, "Assertion failed: {}", assert.raw);
    }

    // 验证变量被成功从高频流中正确提取并写入 context (克服了心跳广播的干扰)
    assert_eq!(context.get("btc_price").as_deref(), Some("62500"));
}

#[tokio::test]
async fn test_websocket_e2e_msgpack_binary() {
    let (ws_url, _server_handle) = start_ws_mock_server().await;

    // 创建临时 HTTP 内容，带有 @websocket、@decoder messagepack、断言以及变量捕获
    let temp_dir = tempfile::TempDir::new().unwrap();
    let http_file = temp_dir.path().join("ws_e2e_msgpack_test.http");

    let content = format!(
        r#"
### WebSocket E2E MsgPack Binary Test
@websocket
@decoder messagepack
@assert body.price == 3200
@assert body.symbol == "ETH"
@capture eth_price from body.price
GET {}

SEND {{ "action": "subscribe_bin" }}
EXPECT {{"event": "ticker"}}
@timeout = 3000
CLOSE
"#,
        ws_url
    );
    std::fs::write(&http_file, content).unwrap();

    let parsed = HttpFileParser::parse_file(&http_file).unwrap();
    let executor = TestExecutor::new();
    let mut context = VariableContext::new();

    let results = executor.execute_all(parsed, &mut context).await.unwrap();
    assert_eq!(results.len(), 1);

    let res = &results[0];
    assert!(
        res.success,
        "WebSocket MsgPack TestResult should be success. Error: {:?}",
        res.error
    );

    // 验证所有的断言均已通过
    assert!(!res.assertions.is_empty(), "Assertions list cannot be empty");
    for assert in &res.assertions {
        assert!(assert.passed, "Assertion failed: {}", assert.raw);
    }

    // 验证变量被从二进制 messagepack 帧中成功解码、匹配并提取到 context 中
    assert_eq!(context.get("eth_price").as_deref(), Some("3200"));
}

// 启动一个能检测握手 Headers 和 Cookies 的 Mock WebSocket 服务器
async fn start_ws_auth_mock_server() -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let ws_url = format!("ws://127.0.0.1:{}", port);

    let handle = tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await {
            let mut auth_ok = false;
            let mut cookie_ok = false;
            
            let callback = |req: &tokio_tungstenite::tungstenite::handshake::client::Request, resp| {
                if let Some(auth_val) = req.headers().get("authorization") {
                    if auth_val.to_str().unwrap().contains("Bearer secret-key-123") {
                        auth_ok = true;
                    }
                }
                if let Some(cookie_val) = req.headers().get("cookie") {
                    if cookie_val.to_str().unwrap().contains("session_id=abc123xyz") {
                        cookie_ok = true;
                    }
                }
                Ok(resp)
            };

            if let Ok(mut ws_stream) = tokio_tungstenite::accept_hdr_async(stream, callback).await {
                let msg_content = format!(
                    "{{\"auth_ok\": {}, \"cookie_ok\": {}}}",
                    auth_ok, cookie_ok
                );
                let _ = ws_stream.send(Message::Text(msg_content)).await;
                let _ = ws_stream.next().await;
            }
        }
    });

    (ws_url, handle)
}

#[tokio::test]
async fn test_websocket_middleware_and_routing() {
    let (ws_url, _server_handle) = start_ws_auth_mock_server().await;

    use rupost::middleware::routing::{RoutingMiddleware, RoutingRule};

    let middleware = RoutingMiddleware::new(vec![
        RoutingRule {
            match_host: "api.rupost.internal".to_string(),
            replace_host: ws_url.replace("ws://", ""),
            inject_headers: {
                let mut map = std::collections::HashMap::new();
                map.insert("Authorization".to_string(), "Bearer secret-key-123".to_string());
                map
            },
        }
    ]);

    let temp_dir = tempfile::TempDir::new().unwrap();
    let http_file = temp_dir.path().join("ws_routing.http");
    let content = r#"
### WS Routing & Authorization middleware test
@websocket
@assert body.auth_ok == true
GET ws://api.rupost.internal/ws
EXPECT auth_ok
@timeout = 3s
CLOSE
"#;
    std::fs::write(&http_file, content).unwrap();

    let parsed = HttpFileParser::parse_file(&http_file).unwrap();
    let executor = TestExecutor::new().with_middleware(std::sync::Arc::new(middleware));
    let mut context = VariableContext::new();

    let results = executor.execute_all(parsed, &mut context).await.unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].success, "WS Upgrade Middleware rewrite and authorization should succeed");
}

#[tokio::test]
async fn test_websocket_cookie_inheritance() {
    let (ws_url, _server_handle) = start_ws_auth_mock_server().await;

    let temp_dir = tempfile::TempDir::new().unwrap();
    let cookie_file = temp_dir.path().join("cookies.json");
    let executor = TestExecutor::with_cookies(cookie_file).unwrap();

    let ws_target_url = url::Url::parse(&ws_url.replace("ws://", "http://")).unwrap();
    let host = ws_target_url.host_str().unwrap();
    let port = ws_target_url.port().unwrap_or(80);
    
    // 利用 CookieMiddleware 自身接口导出状态，确保与底层 cookie_store 完美兼容
    use rupost::middleware::CookieMiddleware;
    let ephemeral = CookieMiddleware::new_ephemeral();
    {
        let store_mutex = ephemeral.cookie_store();
        let mut store = store_mutex.lock().unwrap();
        let cookie = cookie::Cookie::build(("session_id", "abc123xyz"))
            .domain(host)
            .path("/")
            .build();
        let url = url::Url::parse(&format!("http://{}:{}", host, port)).unwrap();
        store.insert_raw(&cookie, &url).unwrap();
    }
    let cookie_json = ephemeral.export_cookie_state().unwrap();
    executor.import_cookie_state(cookie_json).unwrap();

    let http_file = temp_dir.path().join("ws_cookies.http");
    let content = format!(
        r#"
### WS Cookie Inheritance test
@websocket
@assert body.cookie_ok == true
GET {}
EXPECT cookie_ok
@timeout = 3s
CLOSE
"#,
        ws_url
    );
    std::fs::write(&http_file, content).unwrap();

    let parsed = HttpFileParser::parse_file(&http_file).unwrap();
    let mut context = VariableContext::new();

    let results = executor.execute_all(parsed, &mut context).await.unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].success, "WebSocket upgrade handshake should inherit HTTP session cookie");
}

#[tokio::test]
async fn test_websocket_step_assertions_and_captures() {
    let (ws_url, _server_handle) = start_ws_mock_server().await;

    let temp_dir = tempfile::TempDir::new().unwrap();
    let http_file = temp_dir.path().join("ws_step_asserts.http");
    
    let content = format!(
        r#"
### WS Step Assertions & Variable Cascade test
@websocket
GET {}

SEND {{ "action": "subscribe" }}
EXPECT {{"event": "ticker"}}
@timeout = 3s
@assert body.price > 60000
@assert body.symbol == "BTC"
@capture btc_price from body.price

SEND {{ "action": "log_price", "last_price": "{{btc_price}}" }}
EXPECT {{"event": "ticker"}}
@timeout = 3s
CLOSE
"#,
        ws_url
    );
    std::fs::write(&http_file, content).unwrap();

    let parsed = HttpFileParser::parse_file(&http_file).unwrap();
    let executor = TestExecutor::new();
    let mut context = VariableContext::new();

    let results = executor.execute_all(parsed, &mut context).await.unwrap();
    assert_eq!(results.len(), 1);

    let res = &results[0];
    assert!(res.success, "WS Step Assertions & Captures E2E should pass. Error: {:?}", res.error);

    // 验证局部断言结果已经被合并到了结果断言集中，且带有了 stream_event_index 标记！
    assert!(res.assertions.len() >= 2, "Should contain step level assertions");
    for assert in &res.assertions {
        assert!(assert.passed, "Step level assertion failed: {}", assert.raw);
        assert_eq!(assert.stream_event_index, Some(2)); // 它发生在第 2 个动作（EXPECT，1-based 动作顺序中：1是SEND，2是EXPECT）
    }

    // 验证变量被第一步捕获出来，并立刻成功替换到了第二步的 SEND 载荷中
    assert_eq!(context.get("btc_price").as_deref(), Some("62500"));
}

#[tokio::test]
async fn test_websocket_reconnect_and_flush_self_healing() {
    use tokio_tungstenite::tungstenite::Message;

    // 1. 启动第一个 Mock 服务器
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let ws_url = format!("ws://127.0.0.1:{}", port);

    // 服务端只接受连接，然后立刻物理断开（模拟断网物理断线而非发送优雅 Close 帧）
    let server_handle1 = tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await {
            if let Ok(ws_stream) = tokio_tungstenite::accept_async(stream).await {
                drop(ws_stream);
            }
        }
    });

    // 2. 客户端建立逻辑 Session
    use rupost::ws::{WsSession, WsClientConfig, WsFrame, WsFrameType, FrameDirection};
    let config = WsClientConfig {
        url: ws_url.clone(),
        headers: Vec::new(),
        ping_interval: Duration::from_secs(10),
        handshake_timeout: Duration::from_secs(2),
    };

    let session = WsSession::connect(config).await.unwrap();
    
    // 等待第一个服务端将 Socket 关掉，触发客户端的 reconnect
    server_handle1.await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    // 3. 在重连期间，客户端发出一帧 Message A
    let test_frame = WsFrame::new(
        FrameDirection::Outbound,
        WsFrameType::Text,
        "message_a".to_string().into_bytes(),
        0,
    );
    session.send_frame(test_frame).await.unwrap();

    // 4. 在相同端口重新绑定启动第二个 Mock 服务端（模拟服务器恢复）
    let next_listener = TcpListener::bind(format!("127.0.0.1:{}", port)).await.unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    
    let _server_handle2 = tokio::spawn(async move {
        if let Ok((stream, _)) = next_listener.accept().await {
            if let Ok(mut ws_stream) = tokio_tungstenite::accept_async(stream).await {
                if let Some(Ok(Message::Text(txt))) = ws_stream.next().await {
                    let _ = tx.send(txt);
                    let _ = ws_stream.send(Message::Text("received_ok".to_string())).await;
                }
            }
        }
    });

    // 监听逻辑广播流
    let mut client_rx = session.subscribe();

    // 5. 验证：由于指数退避重连，客户端会在约 2 秒内重新连上第二个服务端，并强时序补发 "message_a"！
    let received_by_server = tokio::time::timeout(Duration::from_secs(5), rx).await.unwrap().unwrap();
    assert_eq!(received_by_server, "message_a");

    // 客户端也应该收到服务端回传的确认消息 "received_ok"
    let received_by_client = tokio::time::timeout(Duration::from_secs(5), client_rx.recv()).await.unwrap().unwrap();
    assert_eq!(received_by_client.payload_as_string(), "received_ok");
    
    // 6. 验证滑动历史缓冲区的 push 正常记录
    let history = session.get_history();
    assert!(history.len() >= 2, "History should record send and receive frames");
}

#[tokio::test]
async fn test_websocket_e2e_jsonpath_operators() {
    let (ws_url, _server_handle) = start_ws_mock_server().await;

    let temp_dir = tempfile::TempDir::new().unwrap();
    let http_file = temp_dir.path().join("ws_operators_test.http");

    let content = format!(
        r#"
### WebSocket E2E JSONPath operators test
@websocket
@assert body.price == 62500
GET {}

SEND {{ "action": "subscribe" }}
EXPECT $.event == "ticker"
@timeout = 3000

SEND {{ "action": "log_price" }}
EXPECT $.symbol contains "BT"
@timeout = 3000

SEND {{ "action": "log_price" }}
EXPECT $.price != 1000
@timeout = 3000
CLOSE
"#,
        ws_url
    );
    std::fs::write(&http_file, content).unwrap();

    let parsed = HttpFileParser::parse_file(&http_file).unwrap();
    let executor = TestExecutor::new();
    let mut context = VariableContext::new();

    let results = executor.execute_all(parsed, &mut context).await.unwrap();
    assert_eq!(results.len(), 1);

    let res = &results[0];
    assert!(
        res.success,
        "WebSocket Operators TestResult should be success. Error: {:?}",
        res.error
    );

    // 统计：3个SEND，3个EXPECT，1个CLOSE，总计 7 个 actions 步。
    assert!(res.response.is_some());
    let resp = res.response.as_ref().unwrap();
    assert!(
        resp.body.contains("Run 7 actions."),
        "Actual body: {}", resp.body
    );
}
