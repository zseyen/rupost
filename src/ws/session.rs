use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::collections::VecDeque;
use tokio::sync::{broadcast, mpsc};
use tracing::{info, warn, error};
use crate::ws::frame::WsFrame;
use crate::ws::client::{WsClient, WsClientConfig};

/// Session 的物理连接状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Connected,
    Reconnecting,
    Disconnected,
}

/// 滑动内存限制环形缓冲区，避免高频广播流调试时的内存膨胀
pub struct BoundedFrameBuffer {
    buffer: VecDeque<WsFrame>,
    max_capacity: usize,
}

impl BoundedFrameBuffer {
    pub fn new(max_capacity: usize) -> Self {
        Self {
            buffer: VecDeque::with_capacity(max_capacity),
            max_capacity,
        }
    }

    pub fn push(&mut self, frame: WsFrame) {
        if self.buffer.len() >= self.max_capacity {
            self.buffer.pop_front();
        }
        self.buffer.push_back(frame);
    }

    pub fn get_all(&self) -> Vec<WsFrame> {
        self.buffer.iter().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }
}

/// 逻辑会话代理层，对外暴露出与普通 WsClient 几乎一致的发送与订阅接口
#[derive(Clone)]
pub struct WsSession {
    /// 逻辑发送端：内部写到 SessionManager 的缓冲区队列中
    send_tx: mpsc::Sender<WsFrame>,
    /// 逻辑接收广播端：外部订阅此逻辑 Receiver 即可获取全部物理帧（支持重连跨 Socket 融合）
    broadcast_tx: broadcast::Sender<WsFrame>,
    /// 物理连接状态监控
    state: Arc<Mutex<SessionState>>,
    /// 1000 帧内存上限的滑动历史帧环形缓冲区
    history_buffer: Arc<Mutex<BoundedFrameBuffer>>,
}

impl WsSession {
    /// 发起连接并初始化后台 SessionManager
    pub async fn connect(config: WsClientConfig) -> Result<Self, String> {
        let (send_tx, send_rx) = mpsc::channel::<WsFrame>(128);
        let (broadcast_tx, _) = broadcast::channel::<WsFrame>(1024);
        let state = Arc::new(Mutex::new(SessionState::Connected));
        let history_buffer = Arc::new(Mutex::new(BoundedFrameBuffer::new(1000)));

        // 首次建连：如果首次直接失败，则直接返回错误
        let initial_client = WsClient::connect(config.clone()).await?;

        let session = Self {
            send_tx,
            broadcast_tx: broadcast_tx.clone(),
            state: state.clone(),
            history_buffer: history_buffer.clone(),
        };

        // 启动后台 SessionManager 事件循环
        let manager_broadcast_tx = broadcast_tx;
        let manager_history_buffer = history_buffer;
        tokio::spawn(async move {
            let mut manager = SessionManager::new(
                initial_client,
                config,
                send_rx,
                manager_broadcast_tx,
                state,
                manager_history_buffer,
            );
            manager.run().await;
        });

        Ok(session)
    }

    /// 发送帧，若在重连中，则会缓存到 pending_send_queue
    pub async fn send_frame(&self, frame: WsFrame) -> Result<(), String> {
        self.send_tx
            .send(frame)
            .await
            .map_err(|e| format!("Failed to send frame to SessionManager: {}", e))
    }

    /// 获取融合重连后的广播接收器
    pub fn subscribe(&self) -> broadcast::Receiver<WsFrame> {
        self.broadcast_tx.subscribe()
    }

    /// 获取当前会话状态
    pub fn state(&self) -> SessionState {
        *self.state.lock().unwrap()
    }

    /// 获取当前缓存的历史收发帧数据 (Stage 6)
    pub fn get_history(&self) -> Vec<WsFrame> {
        self.history_buffer.lock().unwrap().get_all()
    }
}

/// 后台会话状态与重连管理者
struct SessionManager {
    /// 当前物理 Client，重连成功后会被替换
    client: WsClient,
    /// 配置模板，用于重新发起握手
    config: WsClientConfig,
    /// 接收逻辑层发来待发送帧的通道
    send_rx: mpsc::Receiver<WsFrame>,
    /// 对外广播收到的物理入站帧
    broadcast_tx: broadcast::Sender<WsFrame>,
    /// 共享的状态指示器
    state: Arc<Mutex<SessionState>>,
    /// 重连期间用于存储发帧请求的缓冲区
    pending_send_queue: VecDeque<WsFrame>,
    /// 滑动历史缓冲区指针
    history_buffer: Arc<Mutex<BoundedFrameBuffer>>,
}

impl SessionManager {
    fn new(
        client: WsClient,
        config: WsClientConfig,
        send_rx: mpsc::Receiver<WsFrame>,
        broadcast_tx: broadcast::Sender<WsFrame>,
        state: Arc<Mutex<SessionState>>,
        history_buffer: Arc<Mutex<BoundedFrameBuffer>>,
    ) -> Self {
        Self {
            client,
            config,
            send_rx,
            broadcast_tx,
            state,
            pending_send_queue: VecDeque::with_capacity(100),
            history_buffer,
        }
    }

    /// 后台管理循环
    async fn run(&mut self) {
        let mut physical_rx = self.client.subscribe();
        
        loop {
            let current_state = *self.state.lock().unwrap();
            
            match current_state {
                SessionState::Connected => {
                    while *self.state.lock().unwrap() == SessionState::Connected {
                        tokio::select! {
                            // 1. 处理上层逻辑写请求
                            maybe_frame = self.send_rx.recv() => {
                                match maybe_frame {
                                    Some(frame) => {
                                        // 记录到历史缓冲区中 (Outbound)
                                        self.history_buffer.lock().unwrap().push(frame.clone());

                                        if let Err(e) = self.client.send_frame(frame.clone()).await {
                                            warn!("Physical send failed: {}. Triggering reconnect.", e);
                                            // 物理发送失败，压入队列头部以待重连后补发
                                            if self.pending_send_queue.len() < 100 {
                                                self.pending_send_queue.push_back(frame);
                                            }
                                            self.transition_to_reconnecting();
                                            break;
                                        }
                                    }
                                    None => {
                                        // WsSession 被释放，退出协程
                                        *self.state.lock().unwrap() = SessionState::Disconnected;
                                        break;
                                    }
                                }
                            }
                            // 2. 处理物理 Socket 收帧，对外进行无缝合并广播
                            maybe_inbound = physical_rx.recv() => {
                                match maybe_inbound {
                                    Ok(frame) => {
                                        // 记录到历史缓冲区中 (Inbound)
                                        self.history_buffer.lock().unwrap().push(frame.clone());

                                        let is_close = matches!(frame.frame_type, crate::ws::WsFrameType::Close);
                                        let _ = self.broadcast_tx.send(frame);
                                        if is_close {
                                            *self.state.lock().unwrap() = SessionState::Disconnected;
                                            break;
                                        }
                                    }
                                    Err(broadcast::error::RecvError::Lagged(c)) => {
                                        warn!("SessionManager lagged by {} frames", c);
                                    }
                                    Err(broadcast::error::RecvError::Closed) => {
                                        warn!("Physical connection closed. Reconnecting.");
                                        self.transition_to_reconnecting();
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                SessionState::Reconnecting => {
                    if self.reconnect_with_backoff().await {
                        physical_rx = self.client.subscribe();
                        self.flush_pending_queue().await;
                    } else {
                        error!("Max reconnect attempts reached. Disconnecting session.");
                        *self.state.lock().unwrap() = SessionState::Disconnected;
                        let _ = self.broadcast_tx.send(WsFrame::new(
                            crate::ws::frame::FrameDirection::Inbound,
                            crate::ws::WsFrameType::Close,
                            Vec::new(),
                            0,
                        ));
                        break;
                    }
                }
                SessionState::Disconnected => {
                    break;
                }
            }
        }
    }

    fn transition_to_reconnecting(&mut self) {
        let mut guard = self.state.lock().unwrap();
        if *guard == SessionState::Connected {
            *guard = SessionState::Reconnecting;
            info!("WsSession transition to RECONNECTING state.");
        }
    }

    /// 执行强时序 Flush 补发
    async fn flush_pending_queue(&mut self) {
        info!("Flushing {} pending frames post-reconnect.", self.pending_send_queue.len());
        while let Some(frame) = self.pending_send_queue.pop_front() {
            if let Err(e) = self.client.send_frame(frame.clone()).await {
                warn!("Secondary disconnection during flush: {}. Re-queueing.", e);
                self.pending_send_queue.push_front(frame);
                self.transition_to_reconnecting();
                break;
            }
        }
    }

    /// 指数退避重连
    async fn reconnect_with_backoff(&mut self) -> bool {
        let max_attempts = 5;
        for attempt in 1..=max_attempts {
            let delay = Duration::from_secs(1 << attempt); // 2s, 4s, 8s, 16s, 32s
            info!("Reconnecting attempt {}/{} in {:?}", attempt, max_attempts, delay);
            
            let sleep_fut = tokio::time::sleep(delay);
            tokio::pin!(sleep_fut);

            loop {
                tokio::select! {
                    _ = &mut sleep_fut => {
                        break;
                    }
                    maybe_frame = self.send_rx.recv() => {
                        match maybe_frame {
                            Some(frame) => {
                                if self.pending_send_queue.len() < 100 {
                                    self.pending_send_queue.push_back(frame);
                                } else {
                                    warn!("Pending send queue overflow during reconnect. Dropped frame.");
                                }
                            }
                            None => {
                                return false;
                            }
                        }
                    }
                }
            }

            match WsClient::connect(self.config.clone()).await {
                Ok(new_client) => {
                    self.client = new_client;
                    *self.state.lock().unwrap() = SessionState::Connected;
                    info!("Successfully reconnected to WebSocket server!");
                    return true;
                }
                Err(e) => {
                    warn!("Reconnection attempt {} failed: {}", attempt, e);
                }
            }
        }
        false
    }
}
