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
                                    if txt.contains("subscribe") {
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
