use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tracing::{error, info};
use crate::Result;

pub struct MockLlmServer {
    port: u16,
}

impl MockLlmServer {
    pub fn new(port: u16) -> Self {
        Self { port }
    }

    /// 启动本地模拟大模型 SSE 输出的服务端
    pub async fn start(&self) -> Result<()> {
        let addr = SocketAddr::from(([127, 0, 0, 1], self.port));
        let listener = TcpListener::bind(addr).await?;
        info!("LLM Mock Server listening on http://{}", addr);

        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    // 1. 读取并丢弃握手 HTTP 头
                    let mut buf = [0u8; 1024];
                    let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut buf).await;

                    // 2. 返回 200 OK text/event-stream 握手响应
                    let handshake = "HTTP/1.1 200 OK\r\n\
                                     Content-Type: text/event-stream\r\n\
                                     Connection: keep-alive\r\n\r\n";
                    if socket.write_all(handshake.as_bytes()).await.is_err() {
                        return;
                    }

                    // 3. 模拟大模型以 50ms 间隔流式输出 Response 帧
                    // 我们准备两组帧：一组用于 OpenAI 格式，一组用于 Anthropic 格式。
                    // 简单起见，根据请求路径决定：包含 `/v1/messages` 的判定为 Anthropic
                    let request_str = String::from_utf8_lossy(&buf);
                    let frames = if request_str.contains("/v1/messages") {
                        // Anthropic 格式流
                        vec![
                            r#"data: {"type": "content_block_delta", "delta": {"text": "Rust "}}"#,
                            r#"data: {"type": "content_block_delta", "delta": {"text": "is "}}"#,
                            r#"data: {"type": "content_block_delta", "delta": {"text": "perfect."}}"#,
                            "data: [DONE]",
                        ]
                    } else {
                        // OpenAI 格式流
                        vec![
                            r#"data: {"choices":[{"delta":{"content":"Rust "}}]}"#,
                            r#"data: {"choices":[{"delta":{"content":"is "}}]}"#,
                            r#"data: {"choices":[{"delta":{"content":"perfect."}}]}"#,
                            "data: [DONE]",
                        ]
                    };

                    for frame in frames {
                        tokio::time::sleep(Duration::from_millis(30)).await;
                        let formatted_frame = format!("{}\n\n", frame);
                        if socket.write_all(formatted_frame.as_bytes()).await.is_err() {
                            break;
                        }
                    }
                });
            }
        });
        Ok(())
    }
}
