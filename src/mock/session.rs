//! 宿主无关的受管 Mock 服务器会话（批三-3：web/TUI 双前端的共享生命周期核心）。
//!
//! 职责：单次绑定（消除 bind-then-close 二次绑定竞态）+ serve 任务派生 +
//! 优雅关停（oneshot，Drop 兜底）+ 热替换（SwappableMatcher）。
//! 前端差异（流量环/DTO/事件转发）留在各自层。

use crate::mock::matcher::{MockMatcher, SwappableMatcher};
use crate::mock::server::axum_adapter::{MockTrafficEvent, serve_matcher_with_events};
use tokio::net::TcpListener;
use tokio::sync::{mpsc::UnboundedSender, oneshot};

/// 受管 Mock 服务器：Drop 即优雅关停（显式 [`shutdown`](Self::shutdown) 可提前）。
pub struct MockServerSession {
    pub port: u16,
    swappable: std::sync::Arc<SwappableMatcher>,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl MockServerSession {
    /// 绑定 `127.0.0.1:{requested_port}`（0 = 自动分配）并派生 serve 任务。
    /// 单次绑定：port 取自 listener 实际地址，杜绝二次绑定竞态。
    pub async fn start(
        requested_port: u16,
        matcher: std::sync::Arc<dyn MockMatcher>,
        enable_cors: bool,
        event_tx: UnboundedSender<MockTrafficEvent>,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", requested_port)).await?;
        let port = listener.local_addr()?.port();
        let swappable = std::sync::Arc::new(SwappableMatcher::new(matcher));
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let swappable_for_task = std::sync::Arc::clone(&swappable);
        tokio::spawn(async move {
            let _ = serve_matcher_with_events(
                listener,
                swappable_for_task,
                enable_cors,
                async move {
                    let _ = shutdown_rx.await;
                },
                Some(event_tx),
            )
            .await;
        });
        Ok(Self {
            port,
            swappable,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    /// 热替换路由匹配器（web reload 用；TUI 单会话不换）
    pub fn swap(&self, next: std::sync::Arc<dyn MockMatcher>) {
        self.swappable.swap(next);
    }

    /// 优雅关停（幂等）；Drop 自动兜底
    pub fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for MockServerSession {
    fn drop(&mut self) {
        self.shutdown();
    }
}
