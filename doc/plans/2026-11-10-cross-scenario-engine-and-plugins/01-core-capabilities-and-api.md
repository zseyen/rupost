# 01 - 微内核自包含基础功能清单与核心 API 规约

- **状态**: Draft (v2.1.0 — 对抗性审查修订稿；评审通过后回标 Approved)
- **归属任务**: `cross-scenario-engine-and-plugins`
- **设计目标**: 锁定发布环境与调试环境下，**0 外部运行时依赖** 即可独立闭环的核心功能与 API。
- **基线标注约定**: 本文档严格区分 **`[已有]`**（现状存在，附 file:line 锚点）与 **`[待建 M<N>]`**（目标态，归属 06 的 Milestone）。禁止把待建能力写成既有能力。

---

## 1. 基础功能范围界定 (Zero-Dependency Invariants)

在没有任何外部解释器（无 Python、Node、Bash）、无外网访问的极端调试环境（如 Alpine / Scratch 容器或跳板机）下，`rupost` 单二进制必须 100% 内置并独立闭环以下 **7 大基础功能**：

> **体积基线（实测口径）**：当前 release 二进制 **18.1 MiB**（`target/release/rupost` = 18,965,952 字节）。本任务体积预算统一为 **≤20MB**（与 05 §1.3 对齐）；v2.0.0 各处"约 15MB"表述作废。构建产物需在 CI 增加 musl 静态链接 target 任务以支撑 "Scratch 容器可用" 叙事（登记于 06）。

```
┌────────────────────────────────────────────────────────────────────────────────────────┐
│                          rupost 单二进制自包含基础功能集合                             │
├────────────────────────────┬─────────────────────────────┬─────────────────────────────┤
│ 1. 原生多协议发包引擎      │ 2. 纯 Rust 内置宏与加签     │ 3. 确定性 DAG 与生命周期    │
│    (HTTP/1/2, WS, SSE)     │    (UUID, Time, HMAC/SHA2)  │    (@depends-on, @teardown) │
├────────────────────────────┼─────────────────────────────┼─────────────────────────────┤
│ 4. 原位网络诊断与微秒时序  │ 5. 多维度充血断言引擎       │ 6. 字节级原文保真请求管道   │
│    (DNS, TCP, TLS, TTFB)   │    (Status, JSONPath, Time) │    (Raw JSON, Content-Type) │
├────────────────────────────┴─────────────────────────────┴─────────────────────────────┤
│ 7. 离线全量数据黑匣子录制 (Execution Snapshot Serialization)                           │
└────────────────────────────────────────────────────────────────────────────────────────┘
```

1. **原生多协议网络管道** `[已有]`：
   * 原生基于 Reqwest（0.13，`Cargo.toml:26`）与 Tokio-Tungstenite（0.24）实现 HTTP/1.1、HTTP/2（reqwest TLS ALPN 隐式能力）、WebSocket 全双工帧交互与 SSE 长连接流式接收（`src/runner/protocol/{http_runner,ws_runner,sse_runner}.rs`）。
2. **纯 Rust 内置宏与基础加签**：
   * **现状** `[已有]`：`src/variable/resolver.rs:20-31` 支持 5 个宏——`$uuid`、`$timestamp`、`$timestamp_ms`、`$timestamp_iso`、`$random_int`（当前**无参数**，固定区间 1~10000）。
   * **待建（MVP，归属 06 M1）** `[待建 M1]`：引入纯 Rust 零系统依赖 `sha2`、`hmac`（Cargo.toml 现无此依赖），实现 `$hmac_sha256(key, msg)`、`$sha256(msg)`；`$random_int` 参数化为 `$random_int(min, max)`。**"0 依赖 CI 闭环加签"叙事以本项交付为前提**。
   * **待建（P2）**：`$md5(msg)`（引入 `md-5`）、`$random_string(len)`、`$random_email`。
   * 验收：常见移动端防刷与 API 加签（HMAC-SHA256 类）在脱机无 Python 环境下 100% 正确签名（04 套件 1 锁定）。
3. **确定性 DAG 拓扑与生命周期清理** `[已有]`：
   * 解析 `@depends-on` 自动编排调用依赖（`src/parser/types.rs:221`）；DAG 拓扑调度（`src/runner/engine/workflow.rs`、`resolver.rs`、`batch.rs`、`parallel.rs`）；
   * 在正向用例失败或熔断退出时，确定性执行 `@teardown` 清理步骤（`src/parser/metadata.rs:35`），确保在生产调试机不遗留垃圾数据。
4. **底层 Socket 级时序探针 (`diagnose`)** `[已有，v2.1.2 口径裁决]`：
   * 内核采集为纳秒级 `Duration`，拆分 DNS 解析、TCP 握手、TLS 握手、TTFB 首包耗时与证书链（`src/http/diagnose.rs:21-30`，`CertInfo` 含 issuer/days_remaining）；
   * **M7 起双层采集**：默认路径 = TimingConnector 单连接真实测量（resolver/connector 双包装点，DNS/TCP 分段 `_us` 真实测量，连接复用为 None，见 `m7-timing-connector-design.md`）；`@diagnose`/`--debug` = 重档 Prober（TLS/证书/HTTP 版本，不变）；
   * **口径锚定**：快照 `_us` 字段 = 真实测量值（非换算）；terminal/HTML 展示层保持 ms 一位小数（格式选择）；`timing_source`（connector/probe_degraded/none）与 `timing_attribution`（exact/approximate）随快照落盘供消费者判读；
   * **并发归属声明**：串行（concurrency=1）精确；并发模式 authority 分桶内近似（ttfb/transfer 永远精确）——性能敏感时序分析建议串行跑；
   * **证书链为 best-effort**：reqwest+rustls 公开 API 不暴露对端证书，需自定义 connector 埋点，未采集时为 `None`，绝不阻塞主流程。
5. **多源多维度断言系统** `[部分已有]`：
   * `[已有]` HTTP 状态码、响应头、JSONPath（`src/utils/jsonpath.rs`）、响应时长阈值（`timing.ttfb < 100`，`src/assertion/types.rs:90` `ValuePath::TimingTtfb`）；
   * `[P2 待建]` 正则断言运算符（`src/assertion/types.rs:100-108` `CompareOp` 现无 matches）。
6. **请求体字节级保真** `[已有]`：
   * JSON 请求体严格遵循"所写即所发"，不抹除空白、不重排键序、不吞没用户自定义的 Content-Type 编码参数（`src/http/request.rs:22` `raw_body`；守护测试 `test_raw_body_preserved_verbatim_without_json_reshuffling`）。
7. **全量数据黑匣子录制（Execution Snapshot）** `[部分已有 → 待建 M1]`：
   * `[已有]` `--save-snapshot` / `--mask-secrets` 落盘链路（`src/history/storage.rs:304` `save_batch_snapshot`）；
   * `[待建 M1]` 按第 02 号文档 Schema v1.0 扩展：时序瀑布、Socket 诊断、顶级错误详情、三态判定（`ExecutionOutcome`）、告警与环境指纹序列化。

---

## 2. 核心 CLI 与 API 规范

### 2.1 针对发布环境（CI/CD）的核心命令
```bash
# 1. 批处理回归测试，输出标准 JUnit XML 并触发快速熔断（严格门禁：禁止插件降级）
rupost test ./tests/ --report-file /ci-cache/junit.xml --fail-fast --strict-plugins

# 2. 携带环境变量与文件覆盖
rupost test suite.http --env staging --var BASE_URL=https://staging.internal.net
```
* **返回码约定**（`[已有]` 0/1 已由 `tests/run_exit_code_test.rs:40-86` 锁定；`2` 为待建目标态）：
  * `0`：全部用例（含 `@teardown`）100% 通过（或在显式开启 `--allow-missing-plugins` 时的 `DegradedPass`）；
  * `1`：存在测试断言失败、网络错误、Teardown 失败，或在 `--strict-plugins` 下发生插件缺失降级；
  * `2` `[待建]`：命令行参数错误（clap 隐式已有）或 AST 语法解析错误。**现状 AST 语法错误经 main 返回 Err 退出 1**（`src/cli/dispatch.rs` 无 `exit(2)`），归位改造与守护用例登记于 06 M1。
  * 该三值契约**仅约束内置命令**；插件子命令退出码按 03 §6.2 原值穿透（信号终止为 `128+signal`），不受此约束。

### 2.2 针对调试环境（现场排障 / 即拷即走）的核心命令
```bash
# 1. 单请求即时探针调试（无需写文件，直接发包并打印微秒级 DNS/TCP/TLS 耗时）
rupost --debug GET http://127.0.0.1:8080/health

# 2. 现场录制数据黑匣子（录制完整报文与诊断指标，供拷回本地分析）
rupost test debug_suite.http --save-snapshot /tmp/incident-dump.json --allow-missing-plugins

# 3. 单请求收敛排查（仅运行文件中指定的单条用例，其余跳过）
rupost test debug_suite.http --only @problematic_endpoint --save-snapshot /tmp/single-dump.json
```
* `--allow-missing-plugins` `[待建 M3]`（全库当前零实现，语义见 03 §2）；
* `--only` 语义裁决 `[已有实现，边界补全]`：自动拉取所选请求的 `@depends-on` 依赖闭包（不拉取则变量缺失必然失败）；`@teardown` 仅执行"被选中请求可达"的清理节点；退出码语义与全量模式一致。

### 2.3 针对开发环境与生态扩展的核心命令 (Plugin & Custom Commands)
```bash
# 1. 执行由插件提供的自定义 CLI 子命令 (如 flamegraph / sigv4)
rupost flamegraph /tmp/incident-dump.json --theme dark
rupost sigv4 --inspect --profile staging

# 2. 插件注册表管理与智能提示 Schema 导出
rupost plugin list                  # 查看已安装扩展命令与生命周期钩子 [待建 M4]
rupost plugin schema                # 导出 JSON Schema 驱动 IDE/VSCode 补全 [待建 M5]

# 3. 模板代码生成 [P2]
rupost template aws-sigv4           # 原位输出最佳实践 @auth 代码块
```
* 自定义命令双模路由 `[待建 M4]`（裁决序见 03 §6.1）；
* **`plugin sync` 裁撤**（v2.1.0）：MVP 无在线源，且静态 manifest 的目录级 stat 新鲜度检测（03 §3.0）已覆盖缓存重建；`plugin sync --regenerate` 作为 `--manifest` 自省的显式高级动作登记 `[P2]`；
* **`template` 现状冲突**：`template` 当前是 `init` 命令的别名（`src/cli/commands/mod.rs:58`，语义为配置初始化），与"插件模板生成"无关；P2 实现插件模板系统时剥离该别名（登记于 06）。

### 2.4 核心编程接口 (Rust Crate API) `[已有，已核实]`
供作为库内嵌到其他运维探针工具中（签名与字段已对照 `src/runner/app.rs:17-87`、`src/runner/mod.rs:11` 核实一致；示例为省略式，完整字段见源码，含 `report_file`/`mask_snapshot`/`only`/`on_failure` 等）：
```rust
use rupost::runner::{run_test_suite, TestSuiteOptions, TestSuiteOutcome};

// 构造零外部依赖的基础测试配置
let options = TestSuiteOptions {
    paths: vec!["/tmp/suite.http".to_string()],
    fail_fast: true,
    save_snapshot: Some("/tmp/dump.json".to_string()),
    ..Default::default()
};

// 纯异步执行，产出带完整时序与报文的结果集
let outcome: TestSuiteOutcome = run_test_suite(options).await?;
assert!(!outcome.has_failure);
```
