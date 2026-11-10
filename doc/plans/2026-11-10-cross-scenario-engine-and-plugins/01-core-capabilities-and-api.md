# 01 - 微内核自包含基础功能清单与核心 API 规约

- **状态**: Approved (Updated with CLI Custom Commands & Registry APIs)
- **归属任务**: `cross-scenario-engine-and-plugins`
- **设计目标**: 锁定发布环境与调试环境下，**0 外部运行时依赖** 即可独立闭环的核心功能与 API。

---

## 1. 基础功能范围界定 (Zero-Dependency Invariants)

在没有任何外部解释器（无 Python、Node、Bash）、无外网访问的极端调试环境（如 Alpine / Scratch 容器或跳板机）下，`rupost` 单二进制（约 15MB）必须 100% 内置并独立闭环以下 **7 大基础功能**：

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

1. **原生多协议网络管道**：
   * 原生基于 Reqwest 与 Tokio-Tungstenite 实现 HTTP/1.1、HTTP/2、WebSocket 全双工帧交互与 SSE 长连接流式接收。
2. **纯 Rust 内置宏与基础加签**：
   * 基于纯 Rust 依赖（`uuid`、`chrono`、`rand`、`sha2`、`hmac`、`md-5`），开箱即用支持：
     - `$uuid`、`$timestamp`、`$timestamp_ms`、`$timestamp_iso`
     - `$random_int(min, max)`、`$random_string(len)`、`$random_email`
     - `$hmac_sha256(key, msg)`、`$sha256(msg)`、`$md5(msg)`
   * 确保常见移动端防刷与 API 加签在脱机无 Python 环境下仍能 100% 正确签名。
3. **确定性 DAG 拓扑与生命周期清理**：
   * 解析 `@depends-on` 自动编排调用依赖；
   * 在正向用例失败或熔断退出时，确定性执行 `@teardown` 清理步骤，确保在生产调试机不遗留垃圾数据。
4. **底层 Socket 级时序探针 (`diagnose`)**：
   * 精准度达微秒级，拆分 DNS 解析、TCP 握手、TLS 握手、TTFB 首包耗时与证书链有效期。
5. **多源多维度断言系统**：
   * 无需外部断言库，内置支持 HTTP 状态码、响应头、JSONPath 查询、正则、响应时长阈值（`timing.ttfb < 100`）。
6. **请求体字节级保真（Phase 0）**：
   * JSON 请求体严格遵循“所写即所发”，不抹除空白、不重排键序、不吞没用户自定义的 Content-Type 编码参数。
7. **全量数据黑匣子录制（Execution Snapshot）**：
   * 原生将请求、响应、时序、环境特征、告警序列化为自包含的 JSON 格式，供离线传输。

---

## 2. 核心 CLI 与 API 规范

### 2.1 针对发布环境（CI/CD）的核心命令
```bash
# 1. 批处理回归测试，输出标准 JUnit XML 并触发快速熔断（严格门禁：禁止插件降级）
rupost test ./tests/ --report-file /ci-cache/junit.xml --fail-fast --strict-plugins

# 2. 携带环境变量与文件覆盖
rupost test suite.http --env staging --var BASE_URL=https://staging.internal.net
```
* **返回码约定**：
  * `0`：全部用例（含 `@teardown`）100% 通过（或在显式开启 `--allow-missing-plugins` 时的 `DegradedPass`）；
  * `1`：存在测试断言失败、网络错误、Teardown 失败，或在 `--strict-plugins` 下发生插件缺失降级；
  * `2`：命令行参数错误或 AST 语法解析错误。

### 2.2 针对调试环境（现场排障 / 即拷即走）的核心命令
```bash
# 1. 单请求即时探针调试（无需写文件，直接发包并打印微秒级 DNS/TCP/TLS 耗时）
rupost --debug GET http://127.0.0.1:8080/health

# 2. 现场录制数据黑匣子（录制完整报文与诊断指标，供拷回本地分析）
rupost test debug_suite.http --save-snapshot /tmp/incident-dump.json --allow-missing-plugins

# 3. 单请求收敛排查（仅运行文件中指定的单条用例，其余跳过）
rupost test debug_suite.http --only @problematic_endpoint --save-snapshot /tmp/single-dump.json
```

### 2.3 针对开发环境与生态扩展的核心命令 (Plugin & Custom Commands)
```bash
# 1. 执行由插件提供的自定义 CLI 子命令 (如 flamegraph / sigv4)
rupost flamegraph /tmp/incident-dump.json --theme dark
rupost sigv4 --inspect --profile staging

# 2. 插件注册表管理与智能提示 Schema 导出
rupost plugin list                  # 查看已安装扩展命令与生命周期钩子
rupost plugin sync                  # 强制更新 ~/.rupost/registry.json 缓存
rupost plugin schema                # 导出 JSON Schema 驱动 IDE/VSCode 补全

# 3. 模板代码生成
rupost template aws-sigv4           # 原位输出最佳实践 @auth 代码块
```

### 2.4 核心编程接口 (Rust Crate API)
供作为库内嵌到其他运维探针工具中：
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
