# Step 5 设计与实施规划：`walkthrough-gaps` 合流 + M7 前置探针 + 蓝本校准

- **状态**: Draft（待对抗审查与用户确认）
- **基线提交**: `uprunrop` / `68dd6783`（Step 4 完成；Step 3 `twmxlwzz` / `af06d05a` 已完成 CLI 插件路由与 TOFU 安全硬化）
- **目标工作区**: `/Users/zsyzzx/project/rust/rupost/.workspace/cross-scenario-plugins`

---

## 1. 目标与边界

Step 5 聚焦两件事并拆分为两个独立原子提交（**Commit 5A** 与 **Commit 5B**）：
1. **Commit 5A（`walkthrough-gaps` 语义合流与 DRY 收口）**：将 `walkthrough-gaps` 工作区尾部尚未进入 `main` 的 2 笔真实业务改进安全合流到当前分支，同步对 5 处重复的变量上下文加载链路做单一事实来源（SSOT）收口，并规避原工作区冲突残余。
2. **Commit 5B（M7 前置探针定案 + 蓝本校准）**：针对 [`m7-timing-connector-design.md`](./m7-timing-connector-design.md) 在 `reqwest 0.13.4` 源码层面暴露的 3 处编译/语义误判编写集成探针实测，确立 M7-A vs M7-B 技术路线并同步修正受影响的 4 份设计文档（暂不动业务请求链路，M7 正式实施归后续 Milestone）。

---

## 2. Commit 5A：`walkthrough-gaps` 合流与 DRY 提炼

### 2.1 分叉差核查与冲突规避裁决

通过 `jj diff` 比对 `heads(::lmwmznky & ::uprunrop)` 到 `lmwmznky`（`abef28f6`），`walkthrough-gaps` 早期 `#1/#3/#5/#6` 主体已并入 `main`，尚未合流的真实有效改动集中于顶部 2 笔提交（`tlwqklvz` + `lmwmznky`，涉及 `--var` / `${VAR}` 静默告警收敛）：

| 源位置（`walkthrough-gaps@lmwmznky`） | 语义改动 | 合流策略与注意事项 |
| :--- | :--- | :--- |
| [`src/variable/config.rs`](../../../src/variable/config.rs) | 新增 `CliVarOutcome { Parsed(k, v), Warn(msg) }`；`build_context` 内对 TOML 引用的未设置 `${VAR}` 去重告警 | **合流并扩展 DRY**：补齐表驱动单测与同变量多键去重测试 |
| [`src/variable/resolver.rs`](../../../src/variable/resolver.rs) | 新增 `VariableResolver::resolve_env_vars_with_warnings`（仅供 `build_context` 使用；热路径 `resolve_env_vars` 与小写 `${x}` 不展开不告警语义不变） | **原样合流**（含 `OnceLock` 正则优化建议：复用已有 regex 定义或保持与 `resolve_env_vars` 同一 regex SSOT） |
| [`src/runner/utils/file_sync.rs`](../../../src/runner/utils/file_sync.rs) | 超限写 `LOG_TRUNCATED_MARKER` 后补 `let _ = file.flush().await;` 防止下次 `metadata` 采样滞后重复写 | **仅摘取 `flush().await`**；**丢弃** `tlwqklvz` 中冲突造成的重复注释垃圾（`"主流程附加信息，写失败不阻断主流程"`） |
| [`src/tui/integrations.rs`](../../../src/tui/integrations.rs) | 原 `lmwmznky` 冲突合并误写为 `.ok().unwrapexpect(...)`（语法损坏/无意义改动） | **禁止合入**；保留当前线清洁代码 |
| [`tests/no_history_test.rs`](../../../tests/no_history_test.rs) | 新增 `test_malformed_var_flag_warns_on_stderr`（校验非法 `--var no_equals_here` 告警走 stderr 且 `--report json` stdout 保持纯净） | **合流**，验证 CLI 侧退出码为 0 与 stderr/stdout 分离 |
| [`src/runner/app.rs`](../../../src/runner/app.rs) / [`src/exporter/mod.rs`](../../../src/exporter/mod.rs) / [`src/tui/app.rs`](../../../src/tui/app.rs) | 原分支在 3 处各自复制 7 行 `for s in &vars { match parse_cli_var_outcome ... }` 循环 | **不按原样复制**——Step 4 已将 `runner/app.rs` 变量加载下沉到 `runner/suite_setup.rs`，改走统一 `ConfigLoader` 入口 |

> **裁决**：鉴于 `tlwqklvz` 存在冲突标记残余与已落地的 `suite_setup.rs` 模块重构，**严禁直接 `jj rebase`/`jj restore` 全量覆盖**；采用精确手工摘取 + TDD 补测完成合流。

### 2.2 DRY 收口设计（消除 5 处重复加载模板）

当前代码库中存在 **5 处高度重复的“5 层变量加载”模板**（均各自调用 `find_and_load()` → 解析变量 → 组装 `.env` 回退规则 → `build_context()` → 汇总 warnings）：
1. [`src/runner/suite_setup.rs`](../../../src/runner/suite_setup.rs)（`load_suite_context`）
2. [`src/exporter/mod.rs`](../../../src/exporter/mod.rs)（`export_collection` 第 3 步）
3. [`src/tui/app.rs`](../../../src/tui/app.rs)（`build_initial_context`）
4. [`src/web/handlers/run.rs`](../../../src/web/handlers/run.rs)（单块执行：`extra_variables` `HashMap` 覆盖）
5. [`src/web/handlers/run.rs`](../../../src/web/handlers/run.rs)（整套执行：空 CLI 变量）

在 [`ConfigLoader`](../../../src/variable/config.rs) 中收敛为如下分层方法（遵守 Clean Architecture 与单一职责，I/O 加载与警告渲染分离）：

```rust
pub enum CliVarOutcome {
    Parsed(String, String),
    Warn(String),
}

impl ConfigLoader {
    /// 解析多个 CLI `--var key=val` 参数；非法项不静默丢弃，收集到 warnings。
    pub fn parse_cli_vars_with_warnings(specs: &[String]) -> (Vec<(String, String)>, Vec<String>);

    /// 统一 5 层上下文装配入口（已具备 K-V 对的场景，如 Web `extra_variables`）。
    pub fn load_context(
        env_name: Option<&str>,
        cli_vars: &[(String, String)],
        env_file: Option<&str>,
    ) -> (VariableContext, Vec<String>);

    /// 统一 5 层上下文装配入口（接收原始 `--var` 字符串切片，供 CLI Runner/Exporter/TUI 共用）。
    pub fn load_context_with_var_specs(
        env_name: Option<&str>,
        var_specs: &[String],
        env_file: Option<&str>,
    ) -> (VariableContext, Vec<String>);
}
```

- **渲染权归各适配层**：`ConfigLoader` 仅返回 `(VariableContext, Vec<String>)`；`suite_setup.rs` 与 `exporter/mod.rs` 写入 `eprintln!("[!] {w}")`，`tui/app.rs` 转入状态栏，`web/handlers/run.rs` 写入 `tracing::warn!`。
- **兼容性**：保留 `parse_cli_var`（如需）或直接令其委托 `parse_cli_var_outcome`，确保已有测试 [`tests/variable_integration_test.rs`](../../../tests/variable_integration_test.rs#L283-L291) 与 [`src/variable/config.rs`](../../../src/variable/config.rs#L447-L460) 无破坏。

### 2.3 Commit 5A TDD 验收清单

1. **Red**：合入 `CliVarOutcome` 决策表单测、`resolve_env_vars_with_warnings` 决策表单测（命中 / 未命中 / 小写 `${x}` 豁免）、TOML 多键引用同一未设置 `${VAR}` 去重单测、`ConfigLoader::load_context` / `load_context_with_var_specs` 等价契约单测，以及 `tests/no_history_test.rs::test_malformed_var_flag_warns_on_stderr`；
2. **Green & Refactor**：实现 `ConfigLoader` 新方法与 `file_sync.rs` 截断 `flush().await`，替换全库 5 处调用点；
3. **Verify**：运行 `cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`。

---

## 3. Commit 5B：`reqwest 0.13.4` M7 前置探针与蓝本校准

### 3.1 `reqwest 0.13.4` 源码核查发现的 3 处蓝图硬伤

直接核对 Cargo 缓存中的 `reqwest-0.13.4` 与 `hyper-util-0.1.19` 源码后，发现原 [`m7-timing-connector-design.md`](./m7-timing-connector-design.md) 的示例代码无法编译且存在关键运行时陷阱：

1. **虚构的 Builder API（无法编译）**：
   - 原蓝图假设存在 `reqwest::Client::builder().resolver(...).connector(...)`。
   - 实际 `reqwest-0.13.4/src/async_impl/client.rs` 中 `ClientBuilder` 仅暴露：
     - `pub fn dns_resolver<R>(mut self, resolver: R) -> ClientBuilder`（`R: reqwest::dns::IntoResolve`）
     - `pub fn connector_layer<L>(mut self, layer: L) -> ClientBuilder`（`L: Layer<BoxedConnectorService>`，`Service<Unnameable, Response = Conn, Error = BoxError>`）
2. **`connector_layer` 的两项内生约束（`connect.rs:613-696` + `sealed`）**：
   - **TLS 被包在内部**：`BoxedConnectorService` 包裹的是 `ConnectorService::connect_with_maybe_proxy` 整体——对于 HTTP，该调用等于 TCP 建连（DNS 在内部发生）；对于 HTTPS，该调用将 **TCP connect + rustls/native-tls 握手（含 ALPN）打包在同一次调用内**，外层 `connector_layer` **无法切分 `tcp_us` 与 `tls_us`**。
   - **目标 `Uri` 与返回 `Conn` 均不可见**：入参 `Unnameable(Uri)` 与响应 `sealed::Conn` 均为私有模块封装，外层 `connector_layer` 无法像原蓝图那样从 `Uri` 读取 authority 进行分桶，也不能自行改写 `Conn::connected().extra(...)`。
3. **跨任务 `task_local!` 与 `http2` Feature 的隐藏风险**：
   - 在 `hyper-util-0.1.19/src/client/legacy/client.rs:417-448` 中，`pool.checkout()` 与 `connect_to()` 并发 `select`：若 `connect_to` 已启动但 `checkout` 先拿到空闲连接，已启动的连接 Future 会被 `self.exec.execute(bg)`（`tokio::spawn`）甩到后台，脱离调用方 Tokio task 上下文（导致 `task_local!` 在该分支失效或无法按请求收集）。
   - 此外，当前 [`Cargo.toml`](../../../Cargo.toml#L26) 配置 `reqwest = { version = "0.13.4", default-features = false, features = ["json", "stream", "default-tls"] }`，未显式列出 `"http2"`；虽依赖树中存在 `h2`，但是否在 `reqwest` 内真正激活 ALPN `h2` 协商需探针实测验证。

### 3.2 M7-A vs M7-B 技术路线对比评估

| 评估维度 | 方案 M7-A（推荐）：`reqwest` 内核 + `dns_resolver` + `connector_layer` | 方案 M7-B：基于 `hyper-util` + `tokio-rustls` 自建 HTTP 客户端 |
| :--- | :--- | :--- |
| **实现路径** | 继续以 [`src/http/client.rs`](../../../src/http/client.rs) 为唯一内核，接入 `TimingResolver`（`reqwest::dns::Resolve`）与 `TimingConnectorLayer`（`tower::Layer`） | 引入 `hyper-util` 直接依赖，手动组装 `GaiResolver -> TcpStream -> tokio-rustls -> HTTP/1.1 & H2` 传输栈替换或旁路 `reqwest` |
| **可观测指标** | `dns_lookup_us` 精确、keep-alive 复用区分（`None` vs `Some`）精确；HTTP 可由 `connect_total - dns` 得精确 `tcp_us`；**HTTPS 默认测得 `(tcp + tls)_us` 合算值**（仅 `--debug`/`@diagnose` 用 `DiagnosticsProber::connect_tcp_staged` 细分 TCP/TLS） | 单连接物理级拆全 `dns_us` / `tcp_us` / `tls_us` 及证书信息 |
| **关联归属机制** | **待探针实测验证**：验证 `task_local`（外加原子槽/FIFO 防并发与 `bg spawn` 保护）或每连接槽位在串行与 10 并发下的表现 | 在自建 Connector 内全权掌控 `Uri` 与连接句柄，可直接按请求上下文打标 |
| **既有能力保留** | 100% 无损保留 CookieJar、代理（HTTP/HTTPS/SOCKS）、重定向、连接池、超时、JSON/Stream Body、H2 ALPN | 需自行补齐或长期维护代理 CONNECT、重定向、Cookie 注入、编码流与 TLS ALPN，重构面极广，明显违背 MVP 与 YAGNI |
| **推荐结论** | **首选**（前提：通过以下 4 项探针门槛，并在文档/Schema 诚实修正 HTTPS 合算语义） | **暂不采用**（仅作为 M7-A 探针暴露出不可接受阻塞缺陷时的极端兜底备选） |

### 3.3 20~40 行前置探针设计（`tests/m7_reqwest_probe_test.rs`）

编写自包含回环集成探针（不修改生产源码 [`src/http/client.rs`](../../../src/http/client.rs)），用断言实证 4 项关键假设：
1. **探针 P1（编译与接口兼容性）**：验证 `reqwest 0.13.4` + 仓库现有 `tower 0.5` 可无报错通过 `Client::builder().dns_resolver(...)` 与 `.connector_layer(...)`（需核对 `reqwest` 与 `rupost` 是否共用了同一 `tower` 主版本及 `Service` trait）。
2. **探针 P2（冷建连 vs Keep-Alive 复用计数与清零）**：对本地回环服务连续发两次 HTTP/1.1 请求——断言第 1 次触发 `dns_resolver` + `connector_layer`（记录 `dns_us` 与 `connect_total_us`），第 2 次复用连接时**两者触发次数均保持为 1**（即可区分“复用 `None`”与“冷建连 `Some(...)`”）。
3. **探针 P3（`task_local!` 在并发与后台连接回收下的归属行为）**：通过本地可控延迟的 TCP 监听/并发 10 请求，实测 `task_local!` 在 `connector_layer` 与 `dns_resolver` 中的传播是否 100% 命中，以及在无 `task_local` 时（例如被 `hyper-util` `bg` spawn 兜底）如何安全降级避免 Panic 或张冠李戴。
4. **探针 P4（H2 ALPN 存活校验）**：启动本地支持 `h2` + `http/1.1` ALPN 的 `tokio-rustls` 服务端（或检查当前 Cargo features），对比“裸 `Client::builder()`”与“挂载 `connector_layer` 的 `Client`”协商出的 `resp.version()`，明确判断是否需要在 [`Cargo.toml`](../../../Cargo.toml#L26) 补回 `"http2"` feature 才支持 HTTP/2。

### 3.4 蓝本校准清单（依据探针实测结果更新）

探针 P1~P4 出结果后，在同一 Commit 5B 同步修订以下规划文档，使文档权威叙述与真实源码 API 对齐：
1. [`m7-timing-connector-design.md`](./m7-timing-connector-design.md)：
   - 删除虚构的 `.resolver().connector()` 与重复解析的 `TcpStream::connect(&str)` 代码；
   - 替换成探针验证可编译的 `dns_resolver` + `connector_layer` 实现方案与关联槽机制；
   - 显式声明 HTTPS 路径下 `connector_layer` 仅能测量 `(tcp + tls)` 合计（不可在同连接无侵入拆分 TCP 与 TLS），并写明在 `TimingWaterfallUs` 中的严谨口径（例如：HTTP 填 `tcp_connect_us`，HTTPS 仅在 `@diagnose`/`--debug` 重档补 `tcp`/`tls` 或新增精确合算语义说明，待定案时与用户二次确认 Schema 变更）；
   - 根据探针 P4 结果，修正 HTTP/2 ALPN 前提条件（包括 `Cargo.toml` feature 现状）。
2. [`interface-freeze-m1.md`](./interface-freeze-m1.md)（§4.10.5）、[`01-core-capabilities-and-api.md`](./01-core-capabilities-and-api.md)（§1.4）与 [`02-artifact-contract-for-plugins.md`](./02-artifact-contract-for-plugins.md)（§2.4）：
   - 同步将“自建 `TimingConnector` returning TCP stream”修正为 `dns_resolver` + `connector_layer` 双钩子与真实字段边界。
3. [`06-mvp-implementation-plan-personas-and-scenarios.md`](./06-mvp-implementation-plan-personas-and-scenarios.md)（§4.5 M7 条目及模块文件索引）：
   - 校准 M7 实现口径；补充 Step 3/Step 4 新增模块锚点（`src/cli/{plugin_route,plugin_exec,runner}.rs`、`src/runner/{suite_setup,executor_plugin}.rs`）。
4. [`p0-p1-remediation-plan.md`](./p0-p1-remediation-plan.md)：
   - 在现有 M6 清单后追加 M7（v2.1.2）对账小节与 Step 1~5 安全/解耦整改闭环记录。

---

## 4. 风险与回滚策略

- **风险 1（`tower` / `http` / `reqwest` 类型不匹配）**：`reqwest` 对 `Unnameable`/`Conn` 是全封闭封装，自定义 `Service` 若需要返回类型对齐只能原地透传 `S::Future` / `BoxFuture`。若探针 P1 发现版本或特征不兼容，立即中止 M7-A 改评 M7-B，不污染生产代码。
- **风险 2（`walkthrough-gaps` 旧文件与 Step 4 新结构差异）**：通过只摘取 `src/variable/*`、`file_sync.rs`、`no_history_test.rs` 并走全量 `cargo test --workspace` + `verify_features.sh` 防止误删 Step 4 既有修复（如并行的 Cookie/插件/Strict 门禁）。
- **原子提交门禁**：Commit 5A 与 Commit 5B 均须各自通过 `cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace` 后执行 `jj commit`。
