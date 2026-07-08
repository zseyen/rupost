use futures_util::{SinkExt, StreamExt};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::tungstenite::protocol::Message;
use tracing::{error, info, warn};

use crate::ws::frame::{FrameDirection, WsFrame, WsFrameType};

/// WsClient 的底层配置
#[derive(Debug, Clone)]
pub struct WsClientConfig {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub ping_interval: Duration,
    pub handshake_timeout: Duration,
}

/// WebSocket 核心连接控制器 (Actor 模式)
#[derive(Clone)]
pub struct WsClient {
    /// 往底层写消息的通道发送端
    write_tx: mpsc::Sender<Message>,
    /// 广播接收消息的通道发送端
    broadcast_tx: broadcast::Sender<WsFrame>,
}

impl WsClient {
    /// 发起连接并初始化后台 Worker
    pub async fn connect(config: WsClientConfig) -> Result<Self, String> {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let mut request = config
            .url
            .as_str()
            .into_client_request()
            .map_err(|e| format!("Invalid WebSocket URL: {}", e))?;

        let headers_mut = request.headers_mut();
        for (k, v) in &config.headers {
            if let (Ok(name), Ok(val)) = (
                tokio_tungstenite::tungstenite::http::header::HeaderName::from_bytes(k.as_bytes()),
                tokio_tungstenite::tungstenite::http::HeaderValue::from_str(v),
            ) {
                headers_mut.insert(name, val);
            }
        }

        // 建立网络连接
        let (ws_stream, response) = tokio::time::timeout(
            config.handshake_timeout,
            tokio_tungstenite::connect_async(request),
        )
        .await
        .map_err(|_| "WebSocket handshake timed out".to_string())?
        .map_err(|e| format!("WebSocket connection failed: {}", e))?;

        info!(
            "WebSocket connected. Handshake HTTP Status: {}",
            response.status()
        );

        let (ws_sink, ws_source) = ws_stream.split();
        let (write_tx, write_rx) = mpsc::channel::<Message>(128);
        let (broadcast_tx, _) = broadcast::channel::<WsFrame>(1024);

        let client = Self {
            write_tx,
            broadcast_tx: broadcast_tx.clone(),
        };

        // 启动后台 Worker 协程
        let worker_broadcast_tx = broadcast_tx;
        let ping_interval = config.ping_interval;
        tokio::spawn(async move {
            Self::run_worker(
                ws_sink,
                ws_source,
                write_rx,
                worker_broadcast_tx,
                ping_interval,
            )
            .await;
        });

        Ok(client)
    }

    /// 发送一个逻辑帧
    pub async fn send_frame(&self, frame: WsFrame) -> Result<(), String> {
        let msg = match frame.frame_type {
            WsFrameType::Text => Message::Text(frame.payload_as_string()),
            WsFrameType::Binary => Message::Binary(frame.payload),
            WsFrameType::Ping => Message::Ping(frame.payload),
            WsFrameType::Pong => Message::Pong(frame.payload),
            WsFrameType::Close => Message::Close(None),
        };
        self.write_tx
            .send(msg)
            .await
            .map_err(|e| format!("Failed to send frame to background worker: {}", e))
    }

    /// 获取广播接收器
    pub fn subscribe(&self) -> broadcast::Receiver<WsFrame> {
        self.broadcast_tx.subscribe()
    }

    /// 后台 Worker 核心事件循环
    async fn run_worker(
        mut ws_sink: futures_util::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
            Message,
        >,
        mut ws_source: futures_util::stream::SplitStream<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
        >,
        mut write_rx: mpsc::Receiver<Message>,
        broadcast_tx: broadcast::Sender<WsFrame>,
        ping_interval: Duration,
    ) {
        let mut ping_timer = tokio::time::interval(ping_interval);
        // 首次立即触发的 tick 予以跳过以保证时间间隔准确
        ping_timer.tick().await;

        let start_time = Instant::now();

        loop {
            tokio::select! {
                // 1. 发送心跳定时器
                _ = ping_timer.tick() => {
                    info!("Sending auto Ping heartbeat to keep connection alive");
                    let ping_msg = Message::Ping(Vec::new());
                    if let Err(e) = ws_sink.send(ping_msg).await {
                        error!("Failed to send auto Ping: {}", e);
                        break;
                    }
                }
                // 2. 外部写请求写入 Socket
                maybe_write = write_rx.recv() => {
                    match maybe_write {
                        Some(msg) => {
                            let is_close = matches!(msg, Message::Close(_));
                            if let Err(e) = ws_sink.send(msg).await {
                                error!("Failed to write message to socket: {}", e);
                                break;
                            }
                            if is_close {
                                info!("Active close requested. Stopping worker.");
                                break;
                            }
                        }
                        None => {
                            warn!("Write channel sender dropped. Stopping worker.");
                            break;
                        }
                    }
                }
                // 3. 读取 Socket 输入帧
                maybe_read = ws_source.next() => {
                    match maybe_read {
                        Some(Ok(msg)) => {
                            let now_ns = start_time.elapsed().as_nanos() as u64;
                            match msg {
                                Message::Text(txt) => {
                                    let frame = WsFrame::new(
                                        FrameDirection::Inbound,
                                        WsFrameType::Text,
                                        txt.into_bytes(),
                                        now_ns
                                    );
                                    let _ = broadcast_tx.send(frame);
                                }
                                Message::Binary(bin) => {
                                    let frame = WsFrame::new(
                                        FrameDirection::Inbound,
                                        WsFrameType::Binary,
                                        bin,
                                        now_ns
                                    );
                                    let _ = broadcast_tx.send(frame);
                                }
                                Message::Ping(payload) => {
                                    // 自动回复 Pong
                                    let pong_msg = Message::Pong(payload);
                                    if let Err(e) = ws_sink.send(pong_msg).await {
                                        error!("Failed to auto reply Pong: {}", e);
                                        break;
                                    }
                                }
                                Message::Pong(_) => {
                                    info!("Received Pong heartbeat response");
                                }
                                Message::Close(_) => {
                                    info!("Received Close frame from remote. Stopping worker.");
                                    let frame = WsFrame::new(
                                        FrameDirection::Inbound,
                                        WsFrameType::Close,
                                        Vec::new(),
                                        now_ns
                                    );
                                    let _ = broadcast_tx.send(frame);
                                    break;
                                }
                                Message::Frame(_) => {}
                            }
                        }
                        Some(Err(e)) => {
                            error!("Error reading from WebSocket socket: {}", e);
                            break;
                        }
                        None => {
                            info!("WebSocket connection closed by remote.");
                            break;
                        }
                    }
                }
            }
        }

        // 尝试关闭连接
        let _ = ws_sink.close().await;
        info!("WebSocket background worker stopped.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ws::frame::WsFrameType;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn test_ws_client_mock() {
        // 1. 启动本地监听
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let local_addr = listener.local_addr().unwrap();
        let ws_url = format!("ws://{}", local_addr);

        // 2. 派生一个服务端 Mock 协程
        tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                let ws_res = tokio_tungstenite::accept_async(stream).await;
                if let Ok(mut ws_stream) = ws_res {
                    // 读取客户端发来的第一条消息并回传
                    if let Some(Ok(Message::Text(msg))) = ws_stream.next().await {
                        let _ = ws_stream
                            .send(Message::Text(format!("echo: {}", msg)))
                            .await;
                    }
                    // 接收 Ping 帧并回复 Pong
                    if let Some(Ok(Message::Ping(payload))) = ws_stream.next().await {
                        let _ = ws_stream.send(Message::Pong(payload)).await;
                    }
                }
            }
        });

        // 3. 客户端发起连接
        let config = WsClientConfig {
            url: ws_url,
            headers: Vec::new(),
            ping_interval: Duration::from_millis(100),
            handshake_timeout: Duration::from_secs(2),
        };

        let client = WsClient::connect(config).await.unwrap();
        let mut rx = client.subscribe();

        // 4. 发送测试数据
        let test_frame = WsFrame::new(
            FrameDirection::Outbound,
            WsFrameType::Text,
            "hello".to_string().into_bytes(),
            0,
        );
        client.send_frame(test_frame).await.unwrap();

        // 5. 验证是否收到 echo 回传帧
        let received = rx.recv().await.unwrap();
        assert_eq!(received.frame_type, WsFrameType::Text);
        assert_eq!(received.payload_as_string(), "echo: hello");
    }
}
