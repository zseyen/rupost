# M7 设计文档：单连接时序采集（Timing Connector）

- **状态**: Approved 设计蓝本（v1.0.0 — 示例代码为实现基线，编译期细节以 reqwest 0.13.4 实际签名为准微调）
- **背景**: R1 缺口——默认 `--save-snapshot` 产出 `timing_waterfall: null`（实测确认），"微秒时序黑匣子"默认形态为空承诺。根因：`RequestTiming` 采集依赖 `DiagnosticsProber` 独立探测连接，仅 `@diagnose`/`--debug` 激活。
- **方案裁决**: 主路径 = TimingResolver + TimingConnector **双包装点**（DNS/TCP 分段真实测量，与请求同连接）；兜底 = Prober 轻量档（`timing_source: "probe_degraded"` 标注）；TLS/证书深度诊断保持 `@diagnose`/`--debug` 重档，与时序采集解耦。

---

## 1. 架构与数据流

```
请求链路（改造后）：
http_runner → reqwest Client（装配 TimingResolver + TimingConnector）
   │
   ├─ TimingResolver::resolve(name)      ← DNS 段计时（getaddrinfo 前后）
   │     └─ pending 区间（authority 分桶）
   ├─ TimingConnector::call(uri)         ← TCP 段计时（tokio TcpStream::connect）
   │     └─ 合并 dns_us + tcp_us → 完成区间（按 authority FIFO）
   ├─ hyper 连接池 → 复用的请求 dns/tcp = None（02 §2 契约天然满足）
   └─ http_runner 秒表 → ttfb/transfer（请求自有，永远精确）
```

**三条已裁决的语义边界**：
1. **authority 分桶**：connect/resolver 打点按 `host:port` 分桶，请求按自身 host 消费——跨 host 错配结构性消灭；同 host 并发错位（池竞争/inflight 等待）为 `approximate`
2. **代理语义**：检测到 `HTTP_PROXY/HTTPS_PROXY` 时 warnings 注明"时序经代理"；CONNECT 隧道内阶段不可见，诚实声明
3. **并发归属**：串行（concurrency=1）= `exact`；并发 = `approximate`（快照字段标注，消费者自行定策略）；`ttfb_us`/`transfer_us` 永远精确（请求自有秒表）

---

## 2. 示例代码：`src/http/timing_connector.rs`（完整实现蓝本）

```rust
//! M7 单连接时序采集（03 §3.1 设计文档；02 §2 Schema timing_source/timing_attribution）。
//!
//! 双包装点：TimingResolver（DNS 段）+ TimingConnector（TCP 段），
//! 与请求共用同一连接，零额外网络开销；连接池复用的请求不触发
//! connector —— dns/tcp = None 与 02 §2 "None = 连接复用" 契约天然吻合。
//!
//! 升级税规避（06 M7 验收）：
//! - 仅依赖 reqwest 两个小 trait（Resolve + Service<Uri>），不包装 HttpConnector
//!   内部类型（TCP connect 自建 tokio::net::TcpStream，hyper-util 内部重组免疫）；
//! - 契约守护测试（H2 存活 + 时序存在性）使 reqwest 升级税显式化（编译期/断言期红）。

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use hyper_util::client::legacy::connect::dns::{GaiResolver, Name};
use hyper_util::rt::TokioIo;

/// 单连接观测区间（DNS + TCP 分段微秒耗时）
#[derive(Debug, Clone, Copy)]
pub struct ConnectSpan {
    pub dns_us: u64,
    pub tcp_us: u64,
}

/// authority 分桶的区间存储：
/// pending（resolver 已记 DNS、connector 未完成 TCP）与 done（完整区间）。
#[derive(Default)]
pub struct TimingSlots {
    pending: Mutex<HashMap<String, VecDeque<PartialSpan>>>,
    done: Mutex<HashMap<String, VecDeque<ConnectSpan>>>,
}

#[derive(Debug)]
struct PartialSpan {
    dns_us: u64,
}

impl TimingSlots {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// resolver 侧：DNS 段完成，挂入 pending（authority 分桶）
    fn push_dns(&self, authority: &str, dns_us: u64) {
        self.pending
            .lock()
            .unwrap()
            .entry(authority.to_string())
            .or_default()
            .push_back(PartialSpan { dns_us });
    }

    /// connector 侧：TCP 段完成，与最早 pending 的 DNS 段配对为完整区间
    fn finish_tcp(&self, authority: &str, tcp_us: u64) {
        if let Some(queue) = self.pending.lock().unwrap().get_mut(authority)
            && let Some(partial) = queue.pop_front()
        {
            self.done
                .lock()
                .unwrap()
                .entry(authority.to_string())
                .or_default()
                .push_back(ConnectSpan { dns_us: partial.dns_us, tcp_us });
        }
    }

    /// http_runner 侧：请求完成后按 authority FIFO 消费一个完整区间。
    /// 并发场景为近似归属（06 M7 验收 `timing_attribution: approximate`）；
    /// 串行（concurrency=1）精确。
    pub fn take_span(&self, authority: &str) -> Option<ConnectSpan> {
        self.done
            .lock()
            .unwrap()
            .get_mut(authority)
            .and_then(|queue| queue.pop_front())
    }
}

// ==================== DNS 段：TimingResolver ====================

/// 包装 GaiResolver（getaddrinfo），在解析前后计时 DNS 段。
/// 仅依赖 reqwest `Resolve` trait（~10 行接口面），不触碰 hyper-util 内部类型。
pub struct TimingResolver {
    inner: GaiResolver,
    slots: Arc<TimingSlots>,
}

impl TimingResolver {
    pub fn new(slots: Arc<TimingSlots>) -> Self {
        Self { inner: GaiResolver::new(), slots }
    }
}

impl tower::Service<Name> for TimingResolver {
    type Response = <GaiResolver as tower::Service<Name>>::Response;
    type Error = <GaiResolver as tower::Service<Name>>::Error;
    type Future = <GaiResolver as tower::Service<Name>>::Future;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        tower::Service::poll_ready(&mut self.inner, cx)
    }

    fn call(&mut self, name: Name) -> Self::Future {
        let authority = name.as_str().to_string();
        let slots = self.slots.clone();
        let start = Instant::now();
        let mut inner = self.inner.clone();
        let future = tower::Service::call(&mut inner, name);
        // GaiResolver::Future 输出 Result<Addrs>；在输出侧补打点并注入 pending
        Box::pin(async move {
            let addrs = future.await?;
            slots.push_dns(&authority, start.elapsed().as_micros() as u64);
            Ok(addrs)
        })
    }
}

// ==================== TCP 段：TimingConnector ====================

/// 自建 TCP connect（tokio::net::TcpStream），计 TCP 段并与 pending DNS 段配对。
/// 不包装 HttpConnector —— hyper-util 内部重组免疫（升级税最小化）。
/// 代价：放弃 happy eyeballs（IPv6/v4 竞争）——排障工具可接受，文档声明。
#[derive(Clone)]
pub struct TimingConnector {
    slots: Arc<TimingSlots>,
}

impl TimingConnector {
    pub fn new(slots: Arc<TimingSlots>) -> Self {
        Self { slots }
    }
}

impl tower::Service<http::Uri> for TimingConnector {
    type Response = TokioIo<TcpStream>;
    type Error = std::io::Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, uri: http::Uri) -> Self::Future {
        let slots = self.slots.clone();
        let authority = format!(
            "{}:{}",
            uri.host().unwrap_or_default(),
            uri.port_u16().unwrap_or(if uri.scheme_str() == Some("https") { 443 } else { 80 })
        );
        Box::pin(async move {
            let start = Instant::now();
            let stream = TcpStream::connect(authority.as_str()).await?;
            let tcp_us = start.elapsed().as_micros() as u64;
            slots.finish_tcp(&authority, tcp_us);
            stream.set_nodelay(true).ok();
            Ok(TokioIo::new(stream))
        })
    }
}
```

> **编译期注记**：reqwest 0.13.4 的 `Resolve`/connector 装配要求 `ClientBuilder` 处于"未启用 TLS 特性包装前"的自定义通道；`Connection` trait 的 ALPN 透传由 `TokioIo<TcpStream>` 的既有实现承担——**M7 开工第一件事是跑 §4 的 H2 存活断言**（防协议静默降级），红则在该测试上迭代，不做任何其他接线。

---

## 3. 接线示例（`src/http/client.rs` 构建处 + 快照消费）

```rust
// Client 构建：resolver + connector 双注入（--timing off 时走原路径）
let timing_slots = TimingSlots::new();
let mut builder = reqwest::Client::builder();
if timing_enabled {
    builder = builder
        .resolver(TimingResolver::new(timing_slots.clone()))
        .connector(TimingConnector::new(timing_slots.clone()));
}
let client = builder.build()?;

// http_runner 请求完成后：按 authority 消费区间 + 合成瀑布（M1-B 既有合成函数）
let span = timing_slots.take_span(&authority_of(&url));
let timing_waterfall = span.map(|s| TimingWaterfallUs {
    dns_lookup_us: Some(s.dns_us),
    tcp_connect_us: Some(s.tcp_us),
    tls_handshake_us: diagnose.and_then(|d| d.tls_handshake_duration.map(|t| t.as_micros() as u64)),
    ttfb_us: ttfb_us,
    transfer_us: transfer_us,
    total_us: s.dns_us + s.tcp_us + ttfb_us + transfer_us,
});
```

快照新字段（02 §2 只增合规）：

```rust
/// 时序采集来源：connector = 单连接真实测量；probe_degraded = 探测连接降级形态；
/// none = 未采集（--timing off）
#[serde(default)]
pub timing_source: TimingSource,       // Connector | ProbeDegraded | None
/// 时序归属精度：exact = 串行精确归属；approximate = 并发近似（authority 分桶内）
#[serde(default)]
pub timing_attribution: TimingAttribution, // Exact | Approximate
```

---

## 4. 验收测试示例（`tests/timing_connector_test.rs` 基线）

```rust
/// 守护 1（升级税保险·最高优先）：H2 存活 —— connector 包装后 ALPN 透传
/// 断裂会导致静默降级 HTTP/1.1，只有协议断言能抓（先跑通本条再做其余接线）
#[tokio::test]
async fn test_h2_negotiation_survives_timing_connector() {
    // 本地 h2 server（如 hyper h2 示例或 nghttpd）+ TimingConnector client
    // 断言 resp.version() == Version::HTTP_2
}

/// 守护 2：默认快照时序存在性（R1 反转即红——修复前为 null）
#[tokio::test]
async fn test_default_snapshot_has_timing() {
    // wiremock + 默认旗标 --save-snapshot
    // 断言 entries[0].timing_waterfall.dns_lookup_us.is_some()
    //      && ttfb_us > 0 && timing_source == "connector"
}

/// 守护 3：时延隔离不变量（DoD 06 §5 性能基线 3）
#[tokio::test]
async fn test_plugin_duration_never_pollutes_ttfb() {
    // mock 插件 sleep 3s + wiremock 固定响应
    // 断言 ttfb_us < 100_000 && plugin_duration_us > 3_000_000
}

/// 守护 4：authority 分桶防跨 host 错配
#[tokio::test]
async fn test_multi_host_no_cross_attribution() {
    // 两个 host 并发请求，断言各自 dns/tcp ∈ 自身 connector 观测区间
}
```

---

## 5. 使用场景与收益矩阵（汇总自评审讨论）

| 场景 | 触发 | 落地前 | 落地后 |
|---|---|---|---|
| 堡垒机 504 排障（场景 C） | 默认 `--save-snapshot` | `timing_waterfall: null` | DNS/TCP/TTFB 微秒拆解入 incident.json，`ai triage-snapshot` 聚类可判"链路 vs 应用" |
| TTFB 归因处方 | 同上 | 凭猜 | TCP 5000ms（网关/路由）≠ TTFB 5000ms（应用慢），处方分叉 |
| CI 性能门禁 | `--report-file junit.xml` | 无时序来源 | junit 带 `ttfb_us`，流水线阈值告警 |
| TUI 实时瀑布 | 默认 | 需 `--debug` | 默认完整瀑布（复用显示"复用"） |
| 火焰图/AI miner 生态 | snapshot 消费 | 空图 | 有米下炊 |
| 深度诊断（证书/协议） | `@diagnose`/`--debug` | 可用 | 不变（重档并存） |
| 隐私极致 | `--timing off` | — | 零打点、无链路指纹 |

---

## 6. 风险登记与残余声明

| 风险 | 缓解 | 残余 |
|---|---|---|
| reqwest trait 演进（0.14 未发布） | 最小 trait 面（Resolve + Service）+ 自建 TCP connect + 三条守护断言 + `timing_source: probe_degraded` 回退 | 大版本重构需重写 connector（约束在一个文件） |
| ALPN/H2 静默降级 | 守护 1 先行（开工第一件事） | 无（断言全覆盖） |
| 并发归属近似 | authority 分桶 + `timing_attribution` 标注 + 串行建议（01 §1.4） | 同 host 高并发错位无法归零（放弃池代价不成比例） |
| 代理场景失真 | warnings 注明"时序经代理" | CONNECT 隧道内阶段不可见（诚实声明） |
| DNS/TCP 双包装点配对竞态 | authority 分桶 FIFO + 并发近似声明 | 无 |

## 7. 工作量与排期

约 340 行（connector/resolver 250 + 接线 40 + 测试 50）+ 文档四处口径修订（本文档即蓝本）。**开工顺序**：守护 1（H2 存活）先跑通 → connector/resolver → 接线 → 守护 2~4 → 文档口径收尾。
