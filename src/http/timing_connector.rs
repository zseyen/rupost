//! M7-A 单连接时序采集（`tokio::task_local!` + `reqwest::dns::Resolve` + `tower::Layer`）。
//!
//! 架构要点：
//! - `TimingDnsResolver` 实现 `reqwest::dns::Resolve`，通过 `tokio::net::lookup_host` 测量 DNS 耗时；
//! - `TimingConnectorLayer` 实现 `tower::Layer<S>`，挂载于 `ClientBuilder::connector_layer`，
//!   测量总建连耗时并扣减当前 task 的 DNS 耗时得到 `tcp_us`（HTTPS 下为诚实的 `tcp+tls` 合并值，
//!   `tls_handshake_us` 默认为 `None`，除非 `--debug` 触发 `DiagnosticsProber` 深度探测）；
//! - `ACTIVE_TIMING_SLOT` 将观测区间绑定到单次 `req.send()` 的异步任务上下文，
//!   结构性消除 IP 字面量、连接错误、重定向、代理与 speculative connect 下的队列泄漏与串台；
//! - 连接池复用请求不触发 `connector_layer`，`ConnectSpan` 为 `None`（对应快照 `dns/tcp = null`）。

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Instant;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use tokio::net::lookup_host;
use tower::{Layer, Service};

/// 单连接底层观测区间（微秒）。
/// - `dns_us`: `None` 表示未发生 DNS 解析（如 IP 字面量或连接复用）。
/// - `tcp_us`: `None` 表示未发生建连（连接池 keep-alive 复用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ConnectSpan {
    pub dns_us: Option<u64>,
    pub tcp_us: Option<u64>,
}

/// 单次请求的 task-local 时序槽位（poison-tolerant Mutex）。
#[derive(Debug, Default)]
pub struct RequestTimingSlot {
    dns_us: Mutex<Option<u64>>,
    span: Mutex<Option<ConnectSpan>>,
}

impl RequestTimingSlot {
    pub fn record_dns(&self, dns_us: u64) {
        let mut guard = self.dns_us.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(dns_us);
    }

    pub fn clear_dns(&self) {
        let mut guard = self.dns_us.lock().unwrap_or_else(|e| e.into_inner());
        *guard = None;
    }

    pub fn record_connect(&self, connect_total_us: u64) {
        let dns_us = {
            let mut guard = self.dns_us.lock().unwrap_or_else(|e| e.into_inner());
            guard.take()
        };
        let tcp_us = Some(match dns_us {
            Some(d) => connect_total_us.saturating_sub(d),
            None => connect_total_us,
        });
        let mut span_guard = self.span.lock().unwrap_or_else(|e| e.into_inner());
        *span_guard = Some(ConnectSpan { dns_us, tcp_us });
    }

    pub fn take_span(&self) -> Option<ConnectSpan> {
        let mut guard = self.span.lock().unwrap_or_else(|e| e.into_inner());
        guard.take()
    }
}

tokio::task_local! {
    pub static ACTIVE_TIMING_SLOT: Arc<RequestTimingSlot>;
}

/// DNS 解析计时包装器：实现 `reqwest::dns::Resolve`。
#[derive(Clone, Default)]
pub struct TimingDnsResolver;

impl Resolve for TimingDnsResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let start = Instant::now();
            let addrs: Vec<SocketAddr> = lookup_host((host.as_str(), 0)).await?.collect();
            let dns_us = start.elapsed().as_micros() as u64;
            let _ = ACTIVE_TIMING_SLOT.try_with(|slot| {
                slot.record_dns(dns_us);
            });
            let iter: Addrs = Box::new(addrs.into_iter());
            Ok(iter)
        })
    }
}

/// 建连计时 Tower Layer：挂载于 `reqwest::ClientBuilder::connector_layer`。
#[derive(Clone, Default)]
pub struct TimingConnectorLayer;

impl<S> Layer<S> for TimingConnectorLayer {
    type Service = TimingConnectorService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TimingConnectorService { inner }
    }
}

/// 建连计时 Tower Service。
#[derive(Clone)]
pub struct TimingConnectorService<S> {
    inner: S,
}

impl<S, Req> Service<Req> for TimingConnectorService<S>
where
    S: Service<Req>,
    S::Future: Send + 'static,
    S::Response: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Req) -> Self::Future {
        let start = Instant::now();
        let fut = self.inner.call(req);
        Box::pin(async move {
            let res = fut.await;
            if res.is_ok() {
                let connect_total_us = start.elapsed().as_micros() as u64;
                let _ = ACTIVE_TIMING_SLOT.try_with(|slot| {
                    slot.record_connect(connect_total_us);
                });
            } else {
                let _ = ACTIVE_TIMING_SLOT.try_with(|slot| {
                    slot.clear_dns();
                });
            }
            res
        })
    }
}

/// 检测环境变量中是否配置了 HTTP/HTTPS/ALL 代理（代理隧道下连接归属为近似值）。
pub fn has_proxy_env() -> bool {
    const KEYS: [&str; 6] = [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ];
    KEYS.iter().any(|k| {
        std::env::var(k)
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_timing_slot_subtracts_dns_from_connect_total() {
        let slot = RequestTimingSlot::default();
        slot.record_dns(120);
        slot.record_connect(500);
        assert_eq!(
            slot.take_span(),
            Some(ConnectSpan {
                dns_us: Some(120),
                tcp_us: Some(380),
            })
        );
        assert_eq!(slot.take_span(), None);
    }

    #[test]
    fn test_request_timing_slot_ip_literal_without_dns() {
        let slot = RequestTimingSlot::default();
        slot.record_connect(250);
        assert_eq!(
            slot.take_span(),
            Some(ConnectSpan {
                dns_us: None,
                tcp_us: Some(250),
            })
        );
    }

    #[test]
    fn test_request_timing_slot_clear_dns_prevents_leak_on_error() {
        let slot = RequestTimingSlot::default();
        slot.record_dns(300);
        slot.clear_dns();
        slot.record_connect(150);
        assert_eq!(
            slot.take_span(),
            Some(ConnectSpan {
                dns_us: None,
                tcp_us: Some(150),
            })
        );
    }
}
