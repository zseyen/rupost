//! 审查补漏（走查缺口 #6d 同款）：WS 帧日志写入失败 warn-once 守护。
//!
//! 独立测试二进制的原因：WS 日志目录在运行时读 `RUPOST_HISTORY_DIR`，
//! 进程级 `set_var` 会与同二进制其他 WS 测试互相污染，故单列文件独占进程。

// 进程级静音历史落盘（tests/common/mod.rs：pre-main 执行，勿删）
mod common;

use futures_util::{SinkExt, StreamExt};
use rupost::parser::HttpFileParser;
use rupost::runner::TestExecutor;
use rupost::variable::VariableContext;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::protocol::Message;

#[tokio::test]
async fn test_ws_frame_log_write_failure_warns_once() {
    // 日志目录父级是一个普通文件：write_log_with_limit 的自动建目录
    // （create_dir_all）必然失败 → 每帧写入都确定性失败
    let temp = tempfile::TempDir::new().unwrap();
    let blocker = temp.path().join("blocker");
    std::fs::write(&blocker, "i-am-a-file").unwrap();
    unsafe {
        std::env::set_var("RUPOST_HISTORY_DIR", blocker.join("history"));
    }

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await
            && let Ok(mut ws) = tokio_tungstenite::accept_async(stream).await
        {
            // 回显文本帧：出站 SEND 一帧 + 入站回显若干帧 = 多次写入尝试
            while let Some(Ok(Message::Text(txt))) = ws.next().await {
                let _ = ws.send(Message::Text(txt)).await;
            }
        }
    });

    let http_file = temp.path().join("ws_log.http");
    std::fs::write(
        &http_file,
        format!(
            r#"
### WS log write failure
@websocket
@assert body.echo contains "hello"
GET ws://127.0.0.1:{port}/

SEND {{"echo": "hello"}}
EXPECT {{"echo": "hello"}}
CLOSE
"#
        ),
    )
    .unwrap();

    let parsed = HttpFileParser::parse_file(&http_file).unwrap();
    let executor = TestExecutor::new();
    let mut context = VariableContext::new();
    let results = executor.execute_all(parsed, &mut context).await.unwrap();
    let _ = server.await;

    let res = &results[0];
    assert!(res.success, "写失败不得影响 WS 测试本身: {:?}", res.error);
    let hits: Vec<_> = res
        .warnings
        .iter()
        .filter(|w| w.contains("写入失败"))
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "WS 写入失败应恰好告警一次: {:?}",
        res.warnings
    );
    assert!(hits[0].contains("WS 流式日志"), "{hits:?}");
    assert!(hits[0].contains("blocker"), "告警应指明目标路径: {hits:?}");
}
