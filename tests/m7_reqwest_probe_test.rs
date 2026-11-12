//! M7-A 探针测试：验证 reqwest 0.13.4 `dns_resolver` + `connector_layer` + `tokio::task_local!`
//! 在真实 HTTP/1.1 keep-alive 场景下的编译签名与行为契约：
//! 1. 首次冷请求（域名 localhost）：记录 dns_us + tcp_us；
//! 2. 第二次连接池复用请求（keep-alive）：connector_layer 不触发 -> ConnectSpan 均为 None；
//! 3. IP 字面量（127.0.0.1）：跳过 DNS 解析 -> dns_us 为 None，tcp_us 为 Some(...)。

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Instant;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, lookup_host};
use tower::{Layer, Service};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ConnectSpan {
    dns_us: Option<u64>,
    tcp_us: Option<u64>,
}

#[derive(Default)]
struct RequestTimingSlot {
    span: Mutex<Option<ConnectSpan>>,
    dns_us: Mutex<Option<u64>>,
}

impl RequestTimingSlot {
    fn record_dns(&self, dns_us: u64) {
        let mut guard = self.dns_us.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(dns_us);
    }

    fn take_dns(&self) -> Option<u64> {
        let mut guard = self.dns_us.lock().unwrap_or_else(|e| e.into_inner());
        guard.take()
    }

    fn record_connect(&self, connect_total_us: u64) {
        let dns_us = self.take_dns();
        let tcp_us = Some(match dns_us {
            Some(d) => connect_total_us.saturating_sub(d),
            None => connect_total_us,
        });
        let mut guard = self.span.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(ConnectSpan { dns_us, tcp_us });
    }

    fn take_span(&self) -> Option<ConnectSpan> {
        let mut guard = self.span.lock().unwrap_or_else(|e| e.into_inner());
        guard.take()
    }
}

tokio::task_local! {
    static ACTIVE_SLOT: Arc<RequestTimingSlot>;
}

#[derive(Clone, Default)]
struct ProbeDnsResolver;

impl Resolve for ProbeDnsResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let start = Instant::now();
            let addrs: Vec<SocketAddr> = lookup_host((host.as_str(), 0)).await?.collect();
            let dns_us = start.elapsed().as_micros() as u64;
            let _ = ACTIVE_SLOT.try_with(|slot| {
                slot.record_dns(dns_us);
            });
            let iter: Addrs = Box::new(addrs.into_iter());
            Ok(iter)
        })
    }
}

#[derive(Clone, Default)]
struct ProbeConnectorLayer;

impl<S> Layer<S> for ProbeConnectorLayer {
    type Service = ProbeConnectorService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ProbeConnectorService { inner }
    }
}

#[derive(Clone)]
struct ProbeConnectorService<S> {
    inner: S,
}

impl<S, Req> Service<Req> for ProbeConnectorService<S>
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
                let _ = ACTIVE_SLOT.try_with(|slot| {
                    slot.record_connect(connect_total_us);
                });
            } else {
                // Clear any stale DNS measurement on connect error
                let _ = ACTIVE_SLOT.try_with(|slot| {
                    let _ = slot.take_dns();
                });
            }
            res
        })
    }
}

async fn spawn_keepalive_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                while let Ok(n) = socket.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                    let resp =
                        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: keep-alive\r\n\r\nok";
                    if socket.write_all(resp.as_bytes()).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
    addr
}

#[tokio::test]
async fn test_reqwest_013_dns_resolver_connector_layer_and_keepalive() {
    let addr = spawn_keepalive_server().await;
    let client = reqwest::Client::builder()
        .dns_resolver(Arc::new(ProbeDnsResolver))
        .connector_layer(ProbeConnectorLayer)
        .build()
        .unwrap();

    // 1. Cold request via hostname `localhost` -> DNS + TCP recorded
    let slot1 = Arc::new(RequestTimingSlot::default());
    let url_host = format!("http://localhost:{}/first", addr.port());
    let resp1 = ACTIVE_SLOT
        .scope(slot1.clone(), async { client.get(&url_host).send().await })
        .await
        .unwrap();
    assert_eq!(resp1.status(), 200);
    let _ = resp1.text().await.unwrap();
    let span1 = slot1
        .take_span()
        .expect("cold request must record ConnectSpan");
    assert!(
        span1.dns_us.is_some(),
        "hostname request must record dns_us"
    );
    assert!(span1.tcp_us.is_some(), "cold request must record tcp_us");

    // 2. Second request to same origin -> pooled keep-alive, connector_layer not invoked
    let slot2 = Arc::new(RequestTimingSlot::default());
    let url_host2 = format!("http://localhost:{}/second", addr.port());
    let resp2 = ACTIVE_SLOT
        .scope(slot2.clone(), async { client.get(&url_host2).send().await })
        .await
        .unwrap();
    assert_eq!(resp2.status(), 200);
    let _ = resp2.text().await.unwrap();
    assert!(
        slot2.take_span().is_none(),
        "pooled keep-alive request must not trigger connector_layer"
    );

    // 3. IP literal `127.0.0.1` -> skips DNS resolver, records tcp_us only
    let slot3 = Arc::new(RequestTimingSlot::default());
    let url_ip = format!("http://127.0.0.1:{}/ip", addr.port());
    let resp3 = ACTIVE_SLOT
        .scope(slot3.clone(), async { client.get(&url_ip).send().await })
        .await
        .unwrap();
    assert_eq!(resp3.status(), 200);
    let _ = resp3.text().await.unwrap();
    let span3 = slot3
        .take_span()
        .expect("IP literal cold request must record ConnectSpan");
    assert!(span3.dns_us.is_none(), "IP literal must not record dns_us");
    assert!(span3.tcp_us.is_some(), "IP literal must record tcp_us");
}
