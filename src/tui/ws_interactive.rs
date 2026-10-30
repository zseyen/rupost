//! WS 交互式调试会话（R6/R7）：TUI 侧独立的 `WsSession` 生命周期。
//!
//! 与 runner 的 `@websocket` 剧本执行零侵入——交互会话由 TUI 直接建连，
//! 收帧经既有 `TuiEvent::WsFrame` 链路回显（复用环形缓冲/渲染/滑窗全链路），
//! 发送走 `WsSession::send_frame`（重连期自动入 pending 队列）。

use crate::tui::event::TuiEvent;
use crate::variable::VariableContext;
use std::time::Duration;
use tokio::sync::mpsc;

/// 活跃的交互会话句柄；drop（置 None）即触发 SessionManager 退出并关闭连接。
/// Arc 包装使 AppState 的 Clone 语义共享同一会话（而非复制连接）。
pub struct WsInteractiveSession {
    pub session: std::sync::Arc<crate::ws::session::WsSession>,
    pub url: String,
}

impl Clone for WsInteractiveSession {
    fn clone(&self) -> Self {
        Self {
            session: std::sync::Arc::clone(&self.session),
            url: self.url.clone(),
        }
    }
}

impl std::fmt::Debug for WsInteractiveSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WsInteractiveSession")
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}

/// 从解析后的 `@websocket` 请求建立常驻交互会话。
/// 变量解析（含快速授权注入，调用侧完成）→ URL 协议转写 → 建连 → 帧转发 task。
pub async fn start_session(
    req: &crate::parser::ParsedRequest,
    var_context: &VariableContext,
    event_tx: mpsc::Sender<TuiEvent>,
) -> Result<WsInteractiveSession, String> {
    // 1. 变量解析（url 与 headers，与 runner 的 temp_req 解析同语义）
    let url = crate::variable::VariableResolver::resolve(&req.url, var_context);
    let mut headers = Vec::new();
    for (k, v) in &req.headers {
        headers.push((
            k.clone(),
            crate::variable::VariableResolver::resolve(v, var_context),
        ));
    }

    // 2. 协议转写 http→ws / https→wss（ws_runner 同一实现）
    let resolved_url = crate::runner::protocol::ws_runner::WsRunner::resolve_ws_url(&url);

    // 3. 建连（首连失败直接错误浮出；断线后的重连由 SessionManager 接管）
    let config = crate::ws::client::WsClientConfig {
        url: resolved_url.clone(),
        headers,
        ping_interval: Duration::from_secs(10),
        handshake_timeout: Duration::from_secs(10),
    };
    let session = std::sync::Arc::new(
        crate::ws::session::WsSession::connect(config)
            .await
            .map_err(|e| e.to_string())?,
    );

    // 4. 帧转发 task：广播帧 → TuiEvent::WsFrame
    //    （manager 只广播入站帧；出站帧由发送侧本地回显）
    let mut rx = session.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(frame) => {
                    let is_send = frame.direction == crate::ws::frame::FrameDirection::Outbound;
                    let content = String::from_utf8_lossy(&frame.payload).into_owned();
                    let _ = event_tx
                        .send(TuiEvent::WsFrame {
                            id: uuid::Uuid::new_v4(),
                            is_send,
                            content,
                            total_lines: 0,
                        })
                        .await;
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    Ok(WsInteractiveSession {
        session,
        url: resolved_url,
    })
}

/// 发送一帧文本到活跃会话（出站帧由调用侧本地回显进视图）
pub async fn send_text(session: &crate::ws::session::WsSession, text: &str) -> Result<(), String> {
    let frame = crate::ws::frame::WsFrame::new(
        crate::ws::frame::FrameDirection::Outbound,
        crate::ws::frame::WsFrameType::Text,
        text.as_bytes().to_vec(),
        0,
    );
    session
        .send_frame(frame)
        .await
        .map_err(|e| format!("WS send failed: {e}"))
}
