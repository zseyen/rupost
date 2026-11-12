# M7 设计文档：单连接时序采集（M7-A `task_local!` Timing Connector）

- **状态**: 已修订（M7-A 方案 — 经 `tests/m7_reqwest_probe_test.rs` 在 `reqwest 0.13.4` 实测编译与 keep-alive 验证通过）
- **背景**: R1 缺口——默认 `--save-snapshot` 产出 `timing_waterfall: null`（实测确认），"微秒时序黑匣子"默认形态为空承诺。根因：`RequestTiming` 采集依赖 `DiagnosticsProber` 独立探测连接，仅 `@diagnose`/`--debug` 激活。
- **方案裁决（M7-A）**:
  - 主路径：`TimingDnsResolver`（`reqwest::dns::Resolve`）+ `TimingConnectorLayer`（`tower::Layer<S>` 挂载于 `ClientBuilder::connector_layer`）+ `tokio::task_local!` 作用域 `ACTIVE_TIMING_SLOT: Arc<RequestTimingSlot>`；
  - 兜底：`DiagnosticsProber` 轻量档（`timing_source: "probe_degraded"`）；
  - 深度诊断：TLS/证书详情仍由 `@diagnose`/`--debug` 触发 `DiagnosticsProber` 提供（`tls_handshake_us: None` 默认不额外建连探测）。

---

## 1. 架构与数据流

```
请求链路（M7-A）：
http_runner → Client::execute_raw_with_timing(request)
   │
   ├─ ACTIVE_TIMING_SLOT.scope(slot, req.send())
   │     ├─ TimingDnsResolver::resolve(name)       ← reqwest::dns::Resolve（tokio::net::lookup_host）
   │     │     └─ 记录 dns_us 到当前 task_local slot（IP 字面量跳过此步）
   │     └─ TimingConnectorLayer / Service::call   ← tower::Layer 包装 reqwest 内部 connector
   │           └─ 测量 connect_total_us：
   │              - HTTP:  tcp_us = connect_total_us.saturating_sub(dns_us)
   │              - HTTPS: tcp_us = connect_total_us.saturating_sub(dns_us)（诚实包含 TCP+TLS；
   │                       tls_handshake_us 默认为 None，除非 --debug DiagnosticsProber 提供）
   │              - 连接失败时清空暂存 dns_us，不残留脏状态
   ├─ hyper 连接池复用 → connector_layer 不触发 → ConnectSpan = None（02 §2 契约：dns/tcp = null）
   └─ http_runner 秒表 → ttfb_us / transfer_us / total_us（永远精确）
```

**为何 M7-A 优于旧版 `.resolver()`/`.connector()` + authority-FIFO `TimingSlots`**：
1. **编译期真实可用**：`reqwest 0.13.4` 公开 API 为 `ClientBuilder::dns_resolver(Arc<dyn Resolve>)` 与 `ClientBuilder::connector_layer(impl Layer<...>)`；保留 `reqwest` 原生 `default-tls`、ALPN/HTTP2、Happy Eyeballs 与代理栈，不破坏 TLS/H2 协商。
2. **结构性消除 FIFO 泄漏**：`RequestTimingSlot` 绑定在单次 `req.send()` 的 `tokio::task_local!` 作用域内，请求结束即消费并随 `Arc` 释放。IP 字面量、连接超时/拒绝、3xx 重定向、代理、speculative connect 均不会向全局队列泄漏或错位消费。
3. **线程安全与抗毒化**：`RequestTimingSlot` 内部使用 `Mutex::lock().unwrap_or_else(|e| e.into_inner())`（poison-tolerant），在 panic 场景下也不会二次 panic。

**语义边界**：
1. **HTTPS 诚实标注**：`connector_layer` 在 reqwest 内部同时包裹 TCP + TLS 建连，故 HTTPS 无 `--debug` 时 `tcp_connect_us` 反映 `tcp+tls` 合并建连耗时，`tls_handshake_us` 为 `None`；启用 `--debug`/`--diagnose` 时由 `DiagnosticsProber` 补充真实 `tls_handshake_us` 与证书指纹。
2. **代理与并发归属（`timing_attribution`）**：检测到 `HTTP_PROXY`/`HTTPS_PROXY`/`ALL_PROXY` 或并行执行（`is_parallel`）时标记 `approximate`；串行无代理请求标记 `exact`。
3. **连接复用语义**：`ConnectSpan == None` 时，请求依然记录 `RequestTiming` 与 `TimingWaterfallUs`（`dns_lookup_us: None`, `tcp_connect_us: None`, `ttfb_us`, `transfer_us`, `total_us`），`timing_source: "connector"`。

---

## 2. 核心组件设计

### 2.1 `src/http/timing_connector.rs`
- `ConnectSpan { pub dns_us: Option<u64>, pub tcp_us: Option<u64> }`
- `RequestTimingSlot`：暂存 `dns_us: Mutex<Option<u64>>` 与 `span: Mutex<Option<ConnectSpan>>`，提供 `record_dns` / `record_connect` / `clear_dns` / `take_span`。
- `tokio::task_local! { pub static ACTIVE_TIMING_SLOT: Arc<RequestTimingSlot>; }`
- `TimingDnsResolver`：实现 `reqwest::dns::Resolve`，内部调用 `tokio::net::lookup_host((host, 0))`，记录微秒耗时至 `ACTIVE_TIMING_SLOT`。
- `TimingConnectorLayer` / `TimingConnectorService<S>`：实现 `tower::Layer<S>` 与 `tower::Service<Req>`，测量 `connect_total_us` 并结合 `dns_us` 计算 `tcp_us`。

### 2.2 `src/http/client.rs`
- `Client::new` 与 `Client::with_cookie_store` 统一通过内部 `base_builder(ua)` 挂载 `.dns_resolver(Arc::new(TimingDnsResolver))` 与 `.connector_layer(TimingConnectorLayer)`。
- 新增 `Client::execute_raw_with_timing(&self, request: Request) -> Result<(reqwest::Response, Duration, Option<ConnectSpan>)>`，用 `ACTIVE_TIMING_SLOT.scope(slot.clone(), req.send())` 包裹发包；保留 `execute_raw` 原签名兼容既有调用方。

### 2.3 `src/http/timing.rs` & `src/history/model.rs`
- 新增：
  - `TimingSource { Connector, ProbeDegraded, None }`（`#[serde(rename_all = "snake_case")]`）
  - `TimingAttribution { Exact, Approximate }`（`#[serde(rename_all = "snake_case")]`）
- `ExecutionSnapshotEntry`（`src/history/model.rs`）新增向后兼容字段：
  - `#[serde(default)] pub timing_source: Option<TimingSource>`
  - `#[serde(default)] pub timing_attribution: Option<TimingAttribution>`
- `TimingWaterfallUs`：支持从 `ConnectSpan` / `RequestTiming` 合成，`dns_lookup_us` 与 `tcp_connect_us` 在连接复用时为 `None`（JSON `null`）。

---

## 3. 验收测试矩阵

1. `tests/m7_reqwest_probe_test.rs`：`reqwest 0.13.4` `dns_resolver` + `connector_layer` + `task_local!` 编译与 keep-alive 行为（冷连接记录 `dns_us`+`tcp_us`，复用连接为 `None`，IP 字面量仅记录 `tcp_us`）。
2. `tests/snapshot_contract_test.rs`：默认 `rupost test --save-snapshot`（无 `--debug`）产出非空 `timing_waterfall`、`timing_source: "connector"`、`timing_attribution: "exact"`；同连接池第二次请求 `dns_lookup_us: null, tcp_connect_us: null` 且 `ttfb_us` 存在。
3. `src/http/timing.rs` 单元测试：`TimingSource` / `TimingAttribution` 序列化契约与 `DiagnosticsProber` 既有用例全绿。
