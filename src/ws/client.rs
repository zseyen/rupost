use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::tungstenite::protocol::Message;
use futures_util::{SinkExt, StreamExt};
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
    /// 配置信息
    config: WsClientConfig,
}

impl WsClient {
    /// 发起连接并初始化后台 Worker
    pub async fn connect(config: WsClientConfig) -> Result<Self, String> {
        let mut request = tokio_tungstenite::tungstenite::handshake::client::Request::builder()
            .method("GET")
            .uri(&config.url);

        for (k, v) in &config.headers {
            request = request.header(k.as_str(), v.as_str());
        }

        let request = request
            .body(())
            .map_err(|e| format!("Failed to build handshake request: {}", e))?;

        // 建立网络连接
        let (ws_stream, response) = tokio::time::timeout(
            config.handshake_timeout,
            tokio_tungstenite::connect_async(request)
        )
        .await
        .map_err(|_| "WebSocket handshake timed out".to_string())?
        .map_err(|e| format!("WebSocket connection failed: {}", e))?;

        info!("WebSocket connected. Handshake HTTP Status: {}", response.status());

        let (ws_sink, ws_source) = ws_stream.split();
        let (write_tx, write_rx) = mpsc::channel::<Message>(128);
        let (broadcast_tx, _) = broadcast::channel::<WsFrame>(1024);

        let client = Self {
            write_tx,
            broadcast_tx: broadcast_tx.clone(),
            config: config.clone(),
        };

        // 启动后台 Worker 协程
        let worker_broadcast_tx = broadcast_tx;
        let ping_interval = config.ping_interval;
        tokio::spawn(async move {
            Self::run_worker(ws_sink, ws_source, write_rx, worker_broadcast_tx, ping_interval).await;
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
            tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
            Message
        >,
        mut ws_source: futures_util::stream::SplitStream<
            tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>
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
