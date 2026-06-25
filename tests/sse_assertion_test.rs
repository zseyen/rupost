use rupost::parser::HttpFileParser;
use rupost::runner::TestExecutor;
use rupost::variable::VariableContext;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

// 辅助工具：启动一个本地的 SSE 模拟服务，按照指定的间隔和行发送 SSE 数据
async fn start_sse_mock_server() -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let uri = format!("http://127.0.0.1:{}", port);

    let handle = tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            // 读取请求头直到空行以清空缓冲区（简单模拟 HTTP 握手响应）
            let mut buf = [0u8; 1024];
            let mut read_bytes = 0;
            loop {
                if let Ok(n) = socket.read(&mut buf[read_bytes..]).await {
                    if n == 0 {
                        break;
                    }
                    read_bytes += n;
                    let request_str = String::from_utf8_lossy(&buf[..read_bytes]);
                    if request_str.contains("\r\n\r\n") || request_str.contains("\n\n") {
                        break;
                    }
                }
            }

            // 返回标准的 HTTP 200 OK 握手响应，Content-Type 为 text/event-stream
            let handshake = "HTTP/1.1 200 OK\r\n\
                             Content-Type: text/event-stream\r\n\
                             Connection: keep-alive\r\n\
                             \r\n";
            socket.write_all(handshake.as_bytes()).await.unwrap();

            // 流式发送事件
            // Event 1
            socket
                .write_all(b"id: 1\nevent: chat\ndata: {\"text\": \"hello\"}\n\n")
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(30)).await;

            // Event 2
            socket
                .write_all(b"id: 2\nevent: chat\ndata: {\"text\": \"world\"}\n\n")
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(30)).await;

            // Event 3
            socket
                .write_all(b"id: 3\nevent: done\ndata: [DONE]\n\n")
                .await
                .unwrap();

            // 保持连接直到客户端主动断开
            let mut temp = [0u8; 10];
            let _ = socket.read(&mut temp).await;
        }
    });

    (uri, handle)
}

#[tokio::test]
async fn test_sse_stream_assertions_and_capture() {
    let (uri, _server_handle) = start_sse_mock_server().await;

    // 创建临时 HTTP 内容，带有 @sse 和流式断言以及捕获
    let temp_dir = tempfile::TempDir::new().unwrap();
    let http_file = temp_dir.path().join("sse_test.http");

    let content = format!(
        r#"
### SSE Stream Test
@sse
@sse_max_events 2
@assert status == 200
@assert stream.event == "chat"
@assert stream.body.text exists
@capture captured_text from stream.body.text
GET {}/stream
Accept: text/event-stream
"#,
        uri
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
        "TestResult should be marked success. Error: {:?}",
        res.error
    );

    // 验证握手断言
    assert!(
        res.assertions
            .iter()
            .any(|a| a.raw == "status == 200" && a.passed)
    );

    // 验证事件流断言（流断言的事件流索引应该存在）
    let stream_event_asserts: Vec<_> = res
        .assertions
        .iter()
        .filter(|a| a.stream_event_index.is_some())
        .collect();
    assert!(!stream_event_asserts.is_empty());
    for assert in &stream_event_asserts {
        assert!(assert.passed, "Stream assertion failed: {}", assert.raw);
    }

    // 验证变量捕获成功（在第二次事件流时 captured_text 应该是 "world"）
    assert_eq!(context.get("captured_text").as_deref(), Some("world"));
}

#[tokio::test]
async fn test_sse_timeout_cutoff() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let uri = format!("http://127.0.0.1:{}", port);

    // 启动一个无限慢速流服务器
    let _server_handle = tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            let handshake = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n";
            let _ = socket.write_all(handshake.as_bytes()).await;

            for i in 0..10 {
                tokio::time::sleep(Duration::from_millis(200)).await;
                if socket
                    .write_all(format!("id: {}\ndata: msg\n\n", i).as_bytes())
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }
    });

    let temp_dir = tempfile::TempDir::new().unwrap();
    let http_file = temp_dir.path().join("sse_timeout.http");

    // 设置 150ms 的超时保护
    let content = format!(
        r#"
### Timeout cut off
@sse
@sse_timeout 150ms
GET {}/stream
"#,
        uri
    );
    std::fs::write(&http_file, content).unwrap();

    let parsed = HttpFileParser::parse_file(&http_file).unwrap();
    let executor = TestExecutor::new();
    let mut context = VariableContext::new();

    let start = std::time::Instant::now();
    let results = executor.execute_all(parsed, &mut context).await.unwrap();
    let elapsed = start.elapsed();

    // 验证执行时长在超时时间上下，没有被长连接卡死
    assert!(
        elapsed < Duration::from_millis(300),
        "Should cutoff early, elapsed: {:?}",
        elapsed
    );
    assert_eq!(results.len(), 1);
    assert!(results[0].success); // 流式超时自动截止，不被视为执行错误
}
