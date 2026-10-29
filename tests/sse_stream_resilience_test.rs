//! SSE 流韧性集成测试（§5.10 缺口：单元层语义上浮到真实网络层）。
//!
//! 锁定的行为契约：
//! 1. 断流尾帧 flush：服务端发出未完结的尾事件后断开连接（EOF/传输层错误
//!    共用同一 flush 分支），尾事件必须照常派发，不得静默丢失；
//! 2. 1MB 内存上限：恶意大事件（>1MB 单行 / >1MB 单事件 data 累积）只截断
//!    不 OOM 不挂死，且事件边界复位后后续正常事件不受影响。
use rupost::parser::HttpFileParser;
use rupost::runner::TestExecutor;
use rupost::variable::VariableContext;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// 完成一次简单的 HTTP 握手（读净请求头，回 200 + text/event-stream）
async fn handshake(socket: &mut tokio::net::TcpStream) {
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
    let handshake = "HTTP/1.1 200 OK\r\n\
                     Content-Type: text/event-stream\r\n\
                     Connection: keep-alive\r\n\
                     \r\n";
    socket.write_all(handshake.as_bytes()).await.unwrap();
}

/// 契约 1：服务端发出未完结尾事件后直接断开，尾事件必须 flush 出来
#[tokio::test]
async fn test_sse_abrupt_disconnect_still_delivers_tail_event() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let uri = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());

    let server = tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            handshake(&mut socket).await;
            // 尾事件无终止空行（"\n\n"）——按 WHATWG 这不是完整事件，
            // 若客户端直接 break 而不 flush，这一帧就静默丢失
            socket
                .write_all(b"id: 9\nevent: done\ndata: {\"text\": \"tail-payload\"}")
                .await
                .unwrap();
            // 留出客户端读到尾帧的时间，然后断流（FIN → 客户端读流结束）
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        // socket 在此 drop → 连接关闭
    });

    let temp = tempfile::TempDir::new().unwrap();
    let http_file = temp.path().join("tail.http");
    std::fs::write(
        &http_file,
        format!(
            r#"### SSE abrupt tail
@sse
@sse_max_events 1
@assert status == 200
@capture tail from stream.body.text
@capture tail_id from stream.id
GET {uri}/stream
"#
        ),
    )
    .unwrap();

    let parsed = HttpFileParser::parse_file(&http_file).unwrap();
    let executor = TestExecutor::new();
    let mut context = VariableContext::new();
    let results = executor.execute_all(parsed, &mut context).await.unwrap();

    server.await.unwrap();
    assert!(
        results[0].success,
        "stream must complete: {:?}",
        results[0].error
    );
    // 契约断言：断流前未完结的尾事件经 flush 照常派发并被捕获
    // 契约断言：断流前未完结的尾事件经 flush 照常派发并被捕获
    assert_eq!(
        context.get("tail").as_deref(),
        Some("tail-payload"),
        "tail event must be flushed and captured, not silently dropped"
    );
    // flush 事件的 id（lastEventId）随事件照常携带
    assert_eq!(context.get("tail_id").as_deref(), Some("9"));
}

/// 契约 2：单事件 data 累积超过 1MB 上限 → 截断且不 OOM；事件边界复位后
/// 后续正常事件完整解析（截断状态不泄漏到下一个事件）
#[tokio::test]
async fn test_sse_giant_event_truncated_and_stream_recovers() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let uri = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());

    let server = tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            handshake(&mut socket).await;
            // 1100 行 × 1KB ≈ 1.1MB 的单事件 data 累积，超过 1MB 上限：
            // 溢出点在 ~1024 行（每行 value 1024 字节 + 1 分隔），此后行被丢弃
            for i in 0..1100 {
                let value = format!("L{i:04}:{}", "a".repeat(1024 - 6));
                socket
                    .write_all(format!("data: {value}\n").as_bytes())
                    .await
                    .unwrap();
            }
            socket.write_all(b"\n\n").await.unwrap(); // 事件边界：溢出复位
            socket
                .write_all(b"data: {\"rec\": \"recovered\"}\n\n")
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });

    let temp = tempfile::TempDir::new().unwrap();
    let http_file = temp.path().join("giant.http");
    std::fs::write(
        &http_file,
        format!(
            r#"### SSE giant event
@sse
@sse_max_events 2
@assert status == 200
@capture rec from stream.body.rec
GET {uri}/stream
"#
        ),
    )
    .unwrap();

    let parsed = HttpFileParser::parse_file(&http_file).unwrap();
    let executor = TestExecutor::new();
    let mut context = VariableContext::new();
    let results = executor.execute_all(parsed, &mut context).await.unwrap();

    server.await.unwrap();
    let res = &results[0];
    assert!(res.success, "stream must complete without OOM/panic: {:?}", res.error);

    // 溢出点前的事件数据保留（前 ~1024 行）
    let body = &res.response.as_ref().unwrap().body;
    assert!(body.contains("L0001"), "pre-overflow data must be kept");
    // 溢出点后的行被丢弃（截断状态生效）
    assert!(
        !body.contains("L1099"),
        "post-overflow data lines must be dropped, not accumulated"
    );
    // 事件边界复位：后续正常事件不受截断状态影响
    assert_eq!(
        context.get("rec").as_deref(),
        Some("recovered"),
        "stream must recover after a truncated giant event"
    );
}

/// 契约 2 补充：无换行残片 buffer 上限（>1MB 无换行 → 截断保尾，处理不挂死）
#[tokio::test]
async fn test_sse_unterminated_giant_line_does_not_hang_or_explode() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let uri = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());

    let server = tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            handshake(&mut socket).await;
            // 2MB 无换行数据（截断保尾后，"data:" 前缀被切掉，该行按未知字段忽略）
            socket
                .write_all(format!("data: {}\n\n", "x".repeat(2 * 1024 * 1024)).as_bytes())
                .await
                .unwrap();
            socket.write_all(b"data: {\"rec\": \"after-giant\"}\n\n").await.unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });

    let temp = tempfile::TempDir::new().unwrap();
    let http_file = temp.path().join("unterminated.http");
    std::fs::write(
        &http_file,
        format!(
            r#"### SSE unterminated giant line
@sse
@sse_max_events 1
@assert status == 200
@capture rec from stream.body.rec
GET {uri}/stream
"#
        ),
    )
    .unwrap();

    let parsed = HttpFileParser::parse_file(&http_file).unwrap();
    let executor = TestExecutor::new();
    let mut context = VariableContext::new();
    let results = executor.execute_all(parsed, &mut context).await.unwrap();

    server.await.unwrap();
    assert!(results[0].success, "{:?}", results[0].error);
    assert_eq!(context.get("rec").as_deref(), Some("after-giant"));
}
