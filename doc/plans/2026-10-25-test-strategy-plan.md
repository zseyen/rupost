# RuPost 全量测试方案 (Test Strategy & Plan)

> **创建日期**: 2026-10-25（v3，2026-10-28 追平现实基线并纳入 AI/单请求重跑/自动诊断新域，修订记录见 §9）
> **性质**: 全局测试策略与功能覆盖矩阵（唯一测试方案事实来源，领域专项规约如 [Cookie 测试规约](file:///Users/zsyzzx/project/rust/rupost/doc/cookie/cookie_test_specification.md) 以本方案为父纲）
> **现状基线**: 全 workspace **551 测试 / 34 套件 / 0 失败** = 主包 543（lib 单测 356 + 主二进制单测 9 + `tests/` 178 集成，29 文件）+ `crates/rupost-ai` 8（client 5 + translator 3）。**⚠️ 默认 `cargo test` 仅覆盖主包 543，crate 的 8 个测试必须 `cargo test --workspace` 才纳入**（实证：workspace root package 默认只跑自身）——§7.1 纪律已随之升级
> **历史基线**: v2 时点 468（317+6+145）；v2→v3 间 P0 缺口 4 项全部落地并超额（13→31 个新测试），详见 §9
> **子代理验证**: 方案成稿后由两个独立子代理（功能清单完整性 / 事实准确性）交叉审查并修订

---

## 1. 测试理念：从项目理念第一性推导

RuPost 的四条产品理念各自推导出一条测试纪律，本方案所有细节均为这四条的下沉执行：

| 项目理念 | 推导出的测试纪律 | 落地机制 |
| :--- | :--- | :--- |
| **第一性原理** | 测试的本质目的是**锁定用户可感知的行为契约**，而非追逐覆盖率数字。每个测试必须能回答"它保护了哪条用户承诺" | §4 覆盖矩阵按"用户故事 → 行为契约 → 测试"三级追溯 |
| **极致简洁 (奥卡姆剃刀)** | 能在纯函数层（L1）验证的绝不上集成层（L2）；每条测试只断言一个行为 | §2 分层金字塔；纯函数下沉单测（如 `nav.rs` 滚动数学、`log_decision` 决策表） |
| **文档即测试** | `examples/` 是活的测试资产，示例与测试共用同一套文件，防止"文档漂移" | `examples/run_all.sh` 一键回归 15 用例；新增示例必须可离线或收敛到稳定服务 |
| **Clean Architecture** | 测试分层与架构分层一一对应：Core 层用纯单测、UseCase 层用隔离集成、Adapter 层用二进制级 E2E | §2 分层模型按 `src/` 三层映射 |

另依据 [研发流程规范](file:///Users/zsyzzx/project/rust/rupost/doc/dev/development_workflow.md) 的**测试设计三原则**（离线隔离 / 数据驱动 / 极值验证），全方案默认执行。

---

## 2. 测试分层模型 (L0–L5，与 Clean Architecture 对齐)

```
L0 静态门禁 ──── Frameworks & Drivers 边界检查（每次提交前强制）
L1 单元测试 ──── Core 层（Entities）：parser/assertion/variable/ws·frame/mock·trie 纯规则
L2 集成测试 ──── UseCase 层：runner/batch/parallel/session，经 Mock 手段隔离 I/O
L3 E2E 脚本 ──── Adapter 层：编译出的 rupost 二进制 + 真实 HTTP 栈（本地自闭环）
L4 专项测试 ──── 对抗性回归 / 并发压力 / 性能基准 / 跨平台
L5 手工探索 ──── 真实外网验证 + TUI 真机冒烟（发布前执行）
```

| 层 | 位置 | 隔离手段 | 规模基线 | 触发时机 |
| :-- | :--- | :--- | :--- | :--- |
| **L0** | CI + 本地 | — | fmt/clippy/MSRV 1.89 双零 | 每次提交（流程规范强制三步） |
| **L1** | 各源文件 `mod tests`（`#[cfg(test)]`） | 内存 Mock（如 `MockHttpClient`）、`InMemory FileRepository`、mockall | 主包 lib 356 + 主二进制 9（含 `cli.rs:621` 全局参数位置契约表） | `cargo test`，每次提交 |
| **L1.5** | `crates/rupost-ai` `mod tests` | mockall（translator 3）+ 逻辑级 client 测试（5：成功/401 状态与体/空 choices/非 JSON/尾斜杠容忍） | 8 个（**仅 `cargo test --workspace` 可见**） | AI 域改动提交时 |
| **L2** | `tests/*.rs`（29 个文件） | wiremock（14 文件）、裸 `TcpListener`（5 文件）、`MockLlmServer`、`TempDir`、ratatui `TestBackend`、**dogfooding 复用 `rupost mock` 当 OpenAI 兼容后端** | 178 个 | `cargo test`，每次提交 |
| **L3** | `tests/verify_features.sh` + `examples/run_all.sh` | 本地 Mock 服务 + 临时沙箱目录，零外网 | 全特性闭环 + 15 示例 | 核心引擎（parser/executor/批量）改动后 + 发版前 |
| **L4** | `tests/adversarial_regression_test.rs`、`benches/`（criterion ×3）、`benches/load_test/` | 原始复现命令、wiremock | 7 对抗 + 3 基准 | Bug 修复同提交；性能改动手动触发 |
| **L5** | [测试与功能验证指引 §2](file:///Users/zsyzzx/project/rust/rupost/doc/guides/testing_guide.md) | 真实外网（`httpbingo.org`、`badssl.com`） | 6 大验证场景 | 发版前 + 网络栈改动后 |

**CI 矩阵**（`.github/workflows/ci.yml`）：fmt / clippy / test 三 Job，test 在 ubuntu / macos / windows 三平台并行。

---

## 3. 测试隔离手段清单（离线隔离的实现全集）

| 手段 | 适用场景 | 使用处 |
| :--- | :--- | :--- |
| `wiremock` | HTTP 服务端行为模拟（状态码、延迟、断流） | 11 个集成测试文件 |
| 裸 `TcpListener` | 需要原始字节级控制的场景（chunked 断流、SSE 尾帧 flush） | 4 个集成测试文件 |
| `MockLlmServer` | 大模型流式响应本地闭环 | `tests/llm_mvp_test.rs` |
| 本地 Mock 服务器（`rupost mock`） | L3 脚本的级联依赖仿真 | `verify_features.sh`、`run_all.sh` |
| `TempDir` 沙箱 | 文件写入、快照、Cookie 落盘、history | 各集成测试 |
| ratatui `TestBackend` | TUI 渲染冒烟（无需真实终端） | `tests/tui_smoke_test.rs` |
| `InMemoryFileRepository` | TUI 写路径端口化测试 | `src/tui/ports.rs` |
| `tokio::time` / `set_delay` | 超时、重连退避、过期 Cookie | 相应单测与集成 |
| `rupost mock` 当 OpenAI 兼容后端（dogfooding） | AI 命令的**无真实 Key 离线闭环**（`ai assert` 等） | `tests/ai_assert_test.rs`、`verify_features.sh` 步骤 7 |
| mockall | crate 侧 trait Mock（`AiClient`） | `crates/rupost-ai/src/translator.rs` |

**外网依赖收敛规则**：自动化测试（L0–L4）**禁止**发起真实公网请求；仅 L5 手工清单允许，且统一收敛到 `httpbingo.org`（功能）与 `badssl.com`（证书异常）。

---

## 4. 功能域覆盖矩阵（20 域 × 现状 × 缺口）

> 状态标记：✅ 覆盖充分 ｜ ⚠️ 覆盖偏薄（有明确缺口） ｜ 🕳 覆盖盲区（见 §6 行动计划）

| # | 功能域 | 核心行为契约 | 现有测试资产 | 状态 |
| :-- | :--- | :--- | :--- | :--- |
| 1 | 文件解析（`.http`/`.md`） | 语法围栏提取、`###` 切分、启发式漏写分隔符自适应、注释前缀剥离、info string | `markdown_integration_test`(8)、`end_to_end_test`(9)、src 内 parser 单测 | ✅ |
| 2 | 断言系统 | 8 运算符矩阵（`== != contains > >= < <= exists`）、None/null 语义、统一 JSONPath、f64 精确相等、引号感知最左扫描 | `adversarial_regression_test`(7)、src 断言单测 | ✅ |
| 3 | 变量与环境 | 四层级联优先级（`--var` > 系统 env > `.env` > `rupost.toml`）、动态变量、未定义警告、`--env` 路径穿越拒绝、`--env-file` 无效路径静默忽略（现状契约） | `variable_integration_test`(13) | ✅ |
| 4 | 变量捕获 | `@capture` 逐条化、失败进 warnings、`global.` 跨文件传递、regex/JSONPath 源 | `variable_integration_test`、批量用例级联 | ⚠️ 缺独立域测试（捕获行为散落在变量/批量测试中） |
| 5 | 批量测试与 DAG | Kahn 拓扑、环路检测、依赖补全、路径合理性校验、State Cloning、Fail-Fast | `batch_testing_test`(9) | ✅ |
| 6 | 并行与 Cookie 隔离 | JoinSet 并发限制、并行分支 Cookie 物理隔离（不落盘）、钻石 DAG 告警 | `cookie_isolation_test`(2)、`batch_testing_test`、`cookie_persistence_test`(2) | ✅ 并行 × `--cookie-file` 契约已锁定（§5.6） |
| 7 | Cookie 管理 | 三模式（AutoPersist/Ephemeral/Disabled）、文件锁、0600、损坏降级、CLI 内联请求的 `--no-cookies`/`--cookie-file`/`--env`→`cookies_<env>.json` 接线 | `cookie_test`(9) + [Cookie 测试规约](file:///Users/zsyzzx/project/rust/rupost/doc/cookie/cookie_test_specification.md) | ✅（CLI 内联接线仅有函数级测试） |
| 8 | 本地 Mock 服务 | Trie 路由（`:id`/`*`/`**`）、多变体条件、兜底分支、Axum 适配 | `mock_integration_test`(3) + src mock 单测 | ✅（单测充分） |
| 9 | 网络诊断 | DNS/TCP/TLS/TTFB 拆分、X.509 临期/过期判定、`--debug`/`--debug-on-failure` 失败自动诊断、`diagnose --report json` 输出 | `diagnose_integration_test`(3)、`timing_diagnostics_test`(2)、`debug_on_failure_test`(4，DBG-2 契约) | ⚠️ `diagnose --report json` 无测试（其余已锁） |
| 10 | SSE 流式 | WHATWG 语义（空 data 帧、event 重置、id 留存）、内存上限、`@sse_timeout`、`@stream_to` | `sse_assertion_test`(2) + src SseRunner 单测 | ⚠️ 集成层偏薄，断流/上限场景需上浮 |
| 11 | LLM 流式调试 | OpenAI/Anthropic 帧规整、`stream.llm.content` 断言、路由重定向 | `llm_mvp_test`(5) | ✅（`crates/rupost-ai` translator 有 mockall 单测，client/error 无测试且 crate 暂无引用方，见 §6 P2） |
| 12 | WebSocket | `@websocket` 剧本（SEND/EXPECT/WAIT/CLOSE）、软匹配、MsgPack、重连自愈、1000 帧上限 | `websocket_integration_test`(8) + src ws 单测 | ✅ |
| 13 | 快照录制与重放 | `--save-snapshot` 录制、`replay --target` 改写、`@timeout` 透传、损坏容错 | `snapshot_replay_test`(4) | ✅（P0-2 达成；结构化 per-entry 结果与 Smart Diff 见 runbook T3/T4） |
| 14 | 请求历史与测试文件生成 | `history list/show/prune/clear` + `export` 桩锁定、`show` 1=最新（help 已修正）、gc、并发写；`generate`（凭据模板化 `{{}}`、语义命名、`--templatize-host`、动态值反向识别 T1/链式溯源 T2 排期中）；顶层 `export` 子命令 | `history_integration_test`(1)、`history_concurrency_test`(1)、`history_cli_test`(5，含 export 桩)、`export_integration_test`(2)、`generate_integration_test`(8) | ✅（P0-1/P0-3 达成并超额；T1/T2 落地时补决策表测试） |
| 15 | CLI 体验 | curl/httpie 解析、单字母别名、**全局参数位置契约（双序皆可 + 内联陷阱，表驱动已锁）**、URL 归一化、`--only`/`path#n` 单请求重跑选择器、日志级别 | `query_params_test`(3)、`auto_routing_test`(2)、`logger_integration_test`(1)、`single_request_test`(8)、`src/cli.rs` 位置契约表 | ✅（P0-4 达成；AGENTS.md §5 已同步修正） |
| 16 | TUI 交互 | MVU 状态机、双 Tab、预折行滚动、`/` 过滤、快速新建三防线、鼠标点击切焦、流式帧增量渲染、`tui <file>` 预加载入口、主题切换（单测覆盖） | `tui_smoke_test`(39) + src 纯函数单测 | ⚠️ 流式渲染（`state/stream.rs`）0 测试；主题轴未进冒烟矩阵；`warnings` 渲染盲区（遗留项 D1） |
| 17 | 模板脚手架 | `init` 三类模板（config/sse/llm）生成、`--list`、防覆盖、`--force` 警告、模板内容完整性、`template` 别名 | `template_test`(9) + L3 脚本 | ✅ |
| 18 | 报告与错误处理 | 终端彩色摘要、JSON 报告 schema（含 warnings）、panic 清零（用户输入→失败结果非崩溃） | `error_test`(4)、JSON 报告断言散落 | ⚠️ JSON schema 契约无独立锁定 |
| 19 | HTTP 客户端行为 | 默认 UA `rupost/1.0.0`、变量配置覆盖、用例 Header 最高优先级覆盖的三级级联 | `user_agent_test`(4) | ✅ |
| 20 | AI 创作辅助 | `ai assert` 自然语言→标准断言（离线 dogfooding、密钥不入盘不入日志、`--raw`）；crate `AiClient` 错误路径；`ai bootstrap`（自举测试，**未实施**：素材脱敏、引擎 lint、降级机械版） | `ai_assert_test`(4)、`verify_features.sh` 步骤 7、crate 8 | ✅（assert 已锁）/ 🕳（bootstrap 随实施交付，见 [自举提案 §8](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-26-ai-bootstrap-proposal.md)） |

---

## 5. 各功能域测试设计要点

> 每域给出：**边界与极值清单**（数据驱动测试的输入维度）与**契约断言**（Given-When-Then 摘要）。已有 ✅ 域仅列守护要点，⚠️/🕳 域列出待补设计。

### 5.1 文件解析
- **极值输入**：空文件、纯注释文件、CRLF/BOM、`.MD` 大写后缀、围栏语言 info string 带参数（```http title=x）、请求 Body 内顶格 `GET http://...`（启发式误切防护）、行前缀 `#`/`//` 与元数据混排。
- **契约**：`rupost t <file>` 对上述每类输入要么产出正确数量的请求，要么产生可读的解析错误，永不 panic。

### 5.2 断言系统
- **运算符矩阵**（表格驱动）：`== != contains > >= < <= exists` × 目标（`status`/`headers.*`/`body.*`/`response.time`）× 值类型（字符串/数字/null/None/带引号字符串）。
- **关键边界**：同位置最长运算符优先（`>=` 胜 `>`）、引号内含运算符字面量、`body.password == null` 字段缺失放行、`status == 700` 非标准码、`1e-20` 极小数、WS `EXPECT` 与 HTTP `@assert` 行为一致。
- **回归锚点**：全部固化在 `adversarial_regression_test.rs`，修复与测试同提交。

### 5.3 变量与环境
- **级联优先级表**（四行，逐对覆盖验证）：`--var` > 系统环境变量 > 本地 `.env` > `rupost.toml [environments.<env>]`；同名变量命名空间过滤（外部 `api_version=v999` 不得渗入）。
- **极值**：未定义 `{{var}}`（警告 + 字面发送）、循环引用、动态变量 `$uuid`/`$timestamp`/`$random_int` 格式、`--env` 含路径穿越字符（报错拒绝）。
- **现状契约锁定（✅ 2026-10-30）**：`--env-file` 指向无效路径或目录时**静默忽略**（`config.rs` 解析失败被 `.ok()` 吞掉，质量加固方案登记的债）——`variable_integration_test::test_env_file_invalid_path_is_silently_ignored` 双场景锁定（不存在路径 / 目录形态）；未来改为告警时此测试先行失败提示同步更新。

### 5.4 变量捕获（已清偿 2026-10-30）
- **`tests/capture_integration_test.rs`**（3 测试）：①逐条降级——一坏一好组合，好的生效、坏的进 `warnings` 且指名道姓、不污染 success 判定、坏变量不得以半成品存在；②变量名含 `from`（`from_id from body.session`）的关键词定位 + regex 源内含 `from`（`regex "from=(\d+)"`，捕获组优先整体匹配）+ header 源同测；③`global.` 前缀跨文件可见性——并行 DAG 依赖链下 `@capture global.x` 经 Arc 共享罐对后继 worker 可见（变量差集克隆机制不携带 `global.*` 键仍可达）。
- **顺带实证的行为边界（现状，非缺陷）**：`from body`（无字段路径的整体捕获）不支持——落入 JSONPath 解析必告警；`{{base}}` 非保留名，URL 渲染残缺时由 `{{base_url}}` 专项校验兜底报错。

### 5.5 批量测试与 DAG
- **拓扑场景**：线性链、Fork/Join、钻石（多父）、成环（报 `Dependency loop detected` 后中断）、缺失依赖自动补全、`@depends-on ../../` 越界拒绝。
- **并行专项**：`--concurrency` 上限生效、Fail-Fast 在并行模式的中断语义、State Cloning 单向性（子节点写变量不回渗父节点）。

### 5.6 并行 × Cookie 组合（已清偿 2026-10-30）
- **`tests/cookie_persistence_test.rs`**（2 测试）锁定完整契约：①串行——会话跨文件内存共享（文件 B 请求携带文件 A 的 Set-Cookie）+ 逐文件落盘（run 返回时 executor 存活即已在盘）；②并行——run 返回时 cookie 文件不存在（worker 全 Ephemeral 从不落盘），主执行器 Drop 兜底后文件可出现但绝不含 worker 新 cookie（兜底只保存主 store）。

### 5.7 Cookie 管理
按 [Cookie 测试规约](file:///Users/zsyzzx/project/rust/rupost/doc/cookie/cookie_test_specification.md) 执行（9 测试：发送/接收/域路径匹配/过期/持久化/双级 Cookie/禁用/损坏降级/10 线程并发）。
- **CLI 内联请求接线**（规约未覆盖，待补函数级之外的一条集成路径）：`rupost <curl/httpie 风格内联请求>` 的三分支——`--no-cookies` 完全无状态、`--cookie-file` 持久化、`--env dev` 按环境隔离为 `cookies_dev.json`。

### 5.8 Mock 服务
- **Trie 矩阵**：精确段、`:id` 捕获渲染回 Body、`*` 单级、`**` 跨级、`**` 中段路由（历史缺陷锚点）。
- **变体评估**：Header/Query/Body(JSONPath) 条件 × Equals/Contains/Exists × None 语义命中；多规则冲突时首条命中优先；无条件规则作兜底。

### 5.9 网络诊断
- **时延拆分**：四段（DNS/TCP/TLS/TTFB）单调性校验（`lookup_host` mock 注入延迟）。
- **证书**：剩余天数计算边界（30 天临界黄色 WARNING / 已过期红色 EXPIRED）、Issuer/SAN 解析。
- **失败自动诊断**（待补）：全局 `--debug` 与 `--debug-on-failure` 注入 `TestSuiteOptions` 后，失败的 HTTP/SSE/WS 用例自动输出时延拆分——`tests/` 目前无任何引用，需 1 个失败用例触发的集成测试锁定。
- **`diagnose --report json`**（待补）：JSON 输出契约与 §5.18 的 schema 契约同守卫。

### 5.10 SSE 流式（待上浮）
- **WHATWG 语义锚点**：空 `data:` 帧不派发、`event:` 跨帧重置、`id:` 跨帧留存、出错分支 flush 尾帧（TcpListener 断流）。
- **资源**：buffer 超过 1MB 上限的截断复位、`@sse_timeout` 中断、`@stream_to` 增量落盘与目录不存在时的错误浮出。
- **已上浮（2026-10-30，`tests/sse_stream_resilience_test.rs`，3 测试）**：①断流尾帧 flush——服务端发未完结尾事件后断连（EOF），尾事件照常派发且 `stream.id`/`stream.body.*` 捕获可用（Err/EOF 两分支共用同一 flush，RST 形态未单测）；②单事件 data 累积超 1MB——溢出点前行保留、溢出点后行丢弃、事件边界复位后正常事件完整解析；③无换行残片超 1MB——截断保尾不挂死，后续事件不受影响。

### 5.11 LLM 流式
OpenAI/Anthropic 双格式的 delta/content_block 帧规整为 `stream.llm.content`；非 LLM 流降级 Generic；`MockLlmServer` 提供本地闭环；路由中间件把外部 host 改写为本地 Mock。

### 5.12 WebSocket
- **剧本矩阵**：SEND→EXPECT 软匹配（高频广播干扰下不雪崩）、WAIT 超时、CLOSE、MsgPack 二进制解码后断言。
- **自愈**：物理断连指数退避 5 次重连、挂起期 100 帧缓冲、重连后 `push_front` 时序补发。
- **资源**：`BoundedFrameBuffer` 1000 帧上限滑动淘汰。

### 5.13 快照录制与重放（v2 缺口已核销 ✅）
P0-2 落地（4 集成测试）：录制→重放往返等价（Host/Port 改写 + Path/Body 透传一次证明）、`@timeout` 透传（<1.5s 计时行为学证明）、损坏快照三分支容错、状态比对失败语义。后续结构化 per-entry 结果（runbook T3）与 Smart Diff（T4）落地时按其验收补测。

### 5.14 请求历史与测试文件生成（v2 缺口已核销 ✅）
P0-1/P0-3 落地：`generate_integration_test`(8，含凭据模板化泄露回归锚点 dc1f048c——生成物凭据一律 `{{变量}}` 化)、`history_cli_test`(5，`show` 1=最新 help 修正同交付、`export` 桩锁定)、`export_integration_test`(2)。`--interactive`（TTY）仍归 L5 手工清单。generate 语义增强（runbook T1 动态值反向识别 / T2 链式溯源）落地时按其五形状/五张决策表补测。

### 5.15 CLI 体验（v2 缺口已核销 ✅）
- **全局参数位置契约**（AGENTS.md CLAP 规则，2026-10-25 实证校准，**已由 `src/cli.rs:621` 表驱动测试锁定**）：子命令模式下 clap v4 对 `global = true` 参数**前后位置皆可**；**真实陷阱在内联模式**（无子命令）——后置全局旗被 `trailing_var_arg` 吞入内联参数且改变解析器选择。AGENTS.md §5 已同步修正。
- **解析器**：curl 风格（`-H`/`-d`/`-X`）、httpie 风格、`strict json` 错误、单字母别名全表（t/h/g/d/m/i/r/l/e/s/p/c）冒烟。
- **URL 归一化**：`normalize_input_url` 决策表（`:3000` 缩写、无 scheme 补全、WS/WSS 原样返回、query 含 `://` 不误判）。
- **单请求重跑选择器**（新增契约，`single_request_test` 8 测试）：`--only <n>` 与 `path#n` 语法、越界/解析失败路径。
- **日志级别**（`logger_integration_test` 守护）：`RUST_LOG` 过滤生效、debug/release 构建的默认级别差异不泄漏调试噪音。

### 5.16 TUI
- **冒烟矩阵**（TestBackend，43 个平铺场景）：布局三态（Wide/Narrow/Stacked）× Tab（Files/History）× 面板切焦 × 固定终端尺寸。**主题轴已进冒烟矩阵**（2026-10-29 tui-fix 批次：Dark/Light × 失败态/SSE 流式/WS 完成态参数化渲染 + buffer 文案断言 + `find_cell_fg` 取色差异断言——若 ui 层硬编码 Dark 色，Light 分支即红；顺带修复 rebuild 管线 `to_string()` 扁平化导致的缓存面板样式丢失，未超宽行现保留 Span 主题取色）。渲染类断言以 `CompletedFrame::buffer` 终端产物为准（消费端守护），不再只断言 state 中间缓存。
- **纯函数层**：`nav` 决策表、`relocate_selection` 表格测试、`filter_matches` 谓词、`log_decision` 决策表、预折行中英文/Emoji 切分、鼠标 `hit_test` 纯几何。
- **安全三防线**：路径穿越拒绝、`create_exclusive` 原子创建、超长文件名截断+哈希（unix 门控）。
- **已补齐（2026-10-29 tui-fix 批次，含双子代理审查修复轮）**：`state/stream.rs` 流式域 0→10 单测（增量折行位点、CJK 显示宽折行、活动行驻留、聚焦自动滚动、WS 方向前缀与 100 帧溢出重建、完成态收敛/warnings 暂存与跨请求清空/Err+warnings 组合）；`rupost tui <file>` 预加载提取为 `app::load_initial_file` 真函数并四场景冒烟（嵌套展开/无参回退/目录节点/不存在路径回退；原冒烟测试为逻辑复刻的假守护，已重写指向真实现）；`TestResult.warnings` 渲染盲区全链路贯通（D1 关闭：事件 `RequestFinished` 增 `warnings` 字段 → `AppState` 暂存（`SendRequest` 清空）→ Response 面板 `Warnings: N` + `⚠` 条目渲染，含 SSE/WS 流式完成态视图首行摘要）；resize 惰性重构路径补冒烟（宽度变化清缓存 → 渲染端按新宽度全量重构、无内容丢失）。

### 5.17 模板脚手架
空目录初始化、重名阻断报错、`--force` 高亮警告不中断、三类模板（config/sse/llm）生成与 `--list` 枚举、模板内容完整性（`environments.dev` 存在性校验）、`template` 别名——L2（9 测试）与 L3 脚本双层守护。

### 5.18 报告与错误处理
- **JSON 报告 schema 契约（已清偿 2026-10-30，`tests/report_schema_test.rs`）**：批量报告顶层 `{file_path, results}` 与逐请求 12 字段（含易漏的 `warnings`/`diagnose_report`）**精确键集合**断言（新增字段也须显式更新契约）；断言子对象 `{raw, passed, message}`；`diagnose --report json` 13 字段精确键集合 + http 明文场景空值形态（`cert_info`/`tls_handshake_duration` 为 null）+ Duration 的 `{secs, nanos}` 序列化形态。
- **现状观察（非缺口）**：`TestResult.timing` 与 `.request`（快照）未纳入 JSON 报告——schema 测试按现状锁定，若未来加入属契约变更。
- **panic 清零纪律**：任何用户输入（状态码、断言、header 名）只允许产生失败的测试结果；每个 panic 修复与 wiremock/TcpListener 复现用例同提交。

### 5.19 HTTP 客户端行为
`user_agent_test` 三级级联：默认 UA `rupost/1.0.0` → 变量配置覆盖 → 用例 `User-Agent` Header 最高优先级覆盖；随域 15 的内联请求冒烟互为补充。

### 5.20 AI 创作辅助（域 20）
- **已锁定**（`ai assert`）：`ai_assert_test` 4 测试——离线 dogfooding 闭环（`rupost mock` 当 OpenAI 兼容后端）、缺 Key 指引、401 错误路径、输出契约；crate 侧 client 5 + translator 3（引号包裹剥离、非法 JSON 降级）；`verify_features.sh` 步骤 7 全离线复现。密钥纪律：不入盘、不进日志、错误信息截断。
- **随 bootstrap 实施交付**（[自举提案 §8](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-26-ai-bootstrap-proposal.md)，L1 密度最高处是信任边界）：
  1. **lint 器表格驱动**（引擎裁决）：合法/非法断言矩阵、capture 断链、批内 `@depends-on` 成环丢弃、`{{MASKED}}` 回扫；
  2. **脱敏器单测**：键名大小写、嵌套 JSON、数组内对象、4KB 截断边界；
  3. **`ai_bootstrap_test`**（二进制级全链离线）：种子历史 → mock AI → 生成 → `rupost t` 生成物全绿；AI 500 降级机械版；非法建议 lint 兜底；防覆盖；
  4. `verify_features.sh` 新步骤（US5 演示闭环）。
- **AI 纪律**（继承两份 PRD）：AI 只在创作期参与，执行期零 AI；AI 不可用降级不塌。

---

## 6. 缺口行动计划（Gap → 优先级）

> 按"每项可独立交付、可砍"原则列项，遵循 MVP 准则。**v2 版 P0 四项已全部落地**（生成域 8 测试、重放 4 测试、历史 CLI 5 测试、CLI 位置契约表，实际交付 31 个新测试——详见 [P0 执行方案](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-25-test-p0-execution.md) 及其落地提交），下表为 v3 重估后的**残余与新增**缺口。

### 6.1 已完成（v2 清单核销，保留作追溯）

| v2 项 | 落地证据 |
| :--- | :--- |
| P0 `generate` 域 0 测试 | `generate_integration_test`(8) + 凭据模板化对抗回归（dc1f048c 泄密修复） |
| P0 快照重放仅 1 测试 | `snapshot_replay_test`(4)：往返等价/timeout 透传/损坏容错 |
| P0 历史子命令 + export 桩 | `history_cli_test`(5)（show help 修正同交付）+ `export_integration_test`(2) |
| P0 CLI 全局参数契约 | `src/cli.rs:621` `test_global_args_position_contract_table`（AGENTS.md §5 同步修正） |
| P1 `--debug-on-failure` 自动诊断 | `debug_on_failure_test`(4)（DBG-2 契约，随 DX 批交付） |

### 6.2 残余与新增缺口

| 优先级 | 缺口项 | 动作 | 验收标准 |
| :--- | :--- | :--- | :--- |
| **P1** | ~~SSE 断流 flush / 1MB 上限未上浮集成层（§5.10）~~（✅ 2026-10-30 清偿：3 测试） | 各上浮 1 个 TcpListener 集成测试 | 集成层可独立复现两个语义 |
| **P1** | ~~捕获域无独立测试文件（§5.4）~~（✅ 2026-10-30 清偿：3 测试全覆盖） | 新增 `capture` 专项测试（逐条降级/`from` 解析/global 克隆） | §5.4 清单全覆盖 |
| **P1** | ~~并行 × `--cookie-file` 行为契约未锁定（§5.6）~~（✅ 2026-10-30 清偿：串行落盘+并行不落盘双契约） | 新增 1 测试锁定"并行分支不落盘"契约 | 契约变更时测试先行失败 |
| **P1** | ~~JSON 报告 schema 无独立契约测试（§5.18）~~（✅ 2026-10-30 清偿：精确键集合断言 ×2） | 新增 schema 断言（字段清单逐项存在性），含 `diagnose --report json` | 手写字段清单漏字段时测试失败 |
| **P1** | AI bootstrap 测试包（§5.20，随实施交付） | lint 表驱动 + 脱敏器边界 + 全链离线集成 + verify 步骤 | [自举提案 §8](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-26-ai-bootstrap-proposal.md) 验收 |
| **P1** | 生成语义 T1/T2 决策表（随 runbook 排期） | 动态值反向识别五形状表 + 链式溯源五张决策表 + wiremock 链式闭环 | [主执行文档 T1/T2](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-27-master-execution-runbook.md) 验收 |
| **P1** | ~~TUI 流式渲染（`state/stream.rs`）0 测试 + `tui <file>` 预加载（§5.16）~~（✅ 2026-10-29 tui-fix 清偿：9 单测 + 预加载真函数三场景冒烟） | 补流式增量渲染单测 + 预加载入口冒烟 | §5.16 待补清单清零 |
| **P1** | ~~TUI `warnings` 渲染盲区（D1 遗留）~~（✅ 2026-10-29 tui-fix 清偿：全链路贯通 + 渲染冒烟） | 功能补齐后补 1 冒烟测试 | 冒烟含含 warning 的结果态 |
| **P2** | ~~`--env-file` 无效路径静默忽略锁定（§5.3）~~（✅ 2026-10-30 清偿：双场景契约测试） | 核对 `variable_integration_test` 是否已含无效路径场景，缺则补 | 契约变更时测试先行失败 |
| **P2** | ~~覆盖率统计未常态化~~（✅ 2026-10-30 清偿：`make coverage` 一键入口，llvm-cov 工具缺失打印安装指引；维持不进 CI 门禁裁决） | 本地可选命令固化；暂不进 CI 门禁 | [测试指引 §4.2](file:///Users/zsyzzx/project/rust/rupost/doc/guides/testing_guide.md) 命令可一键执行 |
| **P2** | criterion 基准无趋势观测（✅ 2026-10-30 验证：3 benches 编译 + variable_resolver_bench 短采样端到端跑通；趋势比对仍为发版前手动仪式，`make gate-all` 旁路可执行） | 发版前手动跑 3 基准比对（`load_generator` 已移出示例，架构债销账） | benches 可运行且耗时无数量级回退 |
| **P2** | ~~主题轴未进 TUI 冒烟矩阵（§5.16）~~（✅ 2026-10-29 tui-fix 清偿：Dark/Light × 三场景参数化冒烟） | TestBackend 冒烟加主题维度参数化 | Dark/Light 各 1 渲染冒烟 |
| **P2** | ~~crate 测试纳入 CI 门禁~~（✅ 2026-10-30 清偿：test 与 clippy 两 job 均升 `--workspace`，实测 38 套件 vs 默认 36） | CI `cargo test` 升级为 `cargo test --workspace`（当前默认跑法漏掉 crate 8 测试） | CI 日志可见 34 套件 |
| **P2** | Windows 平台文件锁/权限差异（✅ 2026-10-30 核对：ci.yml test job 矩阵本就含 ubuntu/macos/windows 三平台，`#[cfg(unix)]` 门控维持，现状达标） | 现有 `#[cfg(unix)]` 门控维持；CI 三平台绿即达标 | CI test Job 三平台 0 失败 |

**P1 合计约 10–14 个新测试；bootstrap/T1/T2 的测试随各自实施切片交付，不单独排期。**

---

## 7. 执行纪律与成功指标

### 7.1 强制纪律（继承研发流程规范）
1. **提交前三步**：`cargo fmt --all -- --check` → `cargo clippy --all-targets --all-features -- -D warnings` → **`cargo test --workspace`** 全绿，缺一不提交。⚠️ 自 AI 激活（crate 获得测试）起，裸 `cargo test` 不覆盖 `crates/rupost-ai` 的 8 个测试（实证：workspace root package 默认只跑主包），涉及 AI 域或发布核验时必须带 `--workspace`。
2. **核心引擎改动**（parser / executor / batch / parallel / generator）：追加跑 `examples/run_all.sh` 与 `tests/verify_features.sh`。
3. **Bug 修复 = 修复 + 对抗性回归测试同一原子提交**（`jj`），杜绝"修了但没锁"。
4. **新增功能 = 功能 + 对应层级测试同一原子提交**，且在 §4 矩阵中登记状态。
5. **AI 域专项**：AI 只在创作期参与；AI 相关测试必须全离线（mock 后端 / mockall），禁止任何测试依赖真实 API Key。

### 7.2 成功指标（本方案的生命周期）
- **基线守护**：全 workspace 测试 0 失败（当前 551/34 套件）、fmt/clippy 双零、`run_all.sh` 15/15、`verify_features.sh` PASS（含 AI dogfooding 步骤）。
- **矩阵收敛**：§4 中 🕳 清零、⚠️ 全部降级为 ✅ 或明确接受为文档化契约。
- **追溯性**：任何测试可沿"用户故事 → 行为契约 → 测试文件"链条追溯；任何 bug 修复可在 `adversarial_regression_test.rs` 或对应域测试中找到锚点（范例：generate 凭据泄露 dc1f048c）。

---

## 8. 文档联动

- 本方案为测试侧唯一父纲；[测试与功能验证指引](file:///Users/zsyzzx/project/rust/rupost/doc/guides/testing_guide.md) 保留为"E2E 手工操作手册"并在开头反向链接本方案。
- 功能实施的任务队列与验收以 [主执行文档（runbook）](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-27-master-execution-runbook.md) 为活状态载体——随实施交付的测试（AI bootstrap / T1 / T2 / T3 / T4 等）的验收标准挂各自任务卡，本方案 §6.2 只登记测试侧视图。
- 领域专项规约（如 [Cookie 测试规约](file:///Users/zsyzzx/project/rust/rupost/doc/cookie/cookie_test_specification.md)）从属于本方案 §4 对应行。
- 进度事实唯一来源仍为 [progress_summary.md](file:///Users/zsyzzx/project/rust/rupost/doc/plans/progress_summary.md)，本方案执行进展按 Stage 6 规范登记其中。

---

## 9. 修订记录

### v2 → v3（2026-10-28，追平现实 + 新域纳入）

v2 定稿后、本次更新前，仓库经两个执行会话大幅推进（AI 激活实施、生成语义批、DX 批、文档治理批），本版将方案追平至 HEAD（`f61b3556`）现实并查漏补缺：

**基线翻新**：468（317+6+145，23 文件）→ **551 / 34 套件**（主包 543 = lib 356 + bin 9 + tests/ 178·29 文件；crate 8）。wiremock 11→14 文件、TcpListener 4→5。
**新发现的方法论缺口**：默认 `cargo test` 不覆盖 workspace 子 crate 测试（实证），§7.1 纪律升级为 `--workspace` 并列 P2 CI 门禁项。

**矩阵（19→20 域）与核销**：
- v2 P0 四项全部核销（§6.1，实际交付 31 测试，超出方案的 13）；`--debug-on-failure`（原 P1）随 DX 批落地（`debug_on_failure_test` 4）。
- 域 13/14/15 从 🕳/⚠️ 翻 ✅；域 14 吸收 generate 凭据模板化/语义命名/`--templatize-host` 新契约与 T1/T2 排期指针。
- **新增域 20 AI 创作辅助**：`ai assert` 已锁（`ai_assert_test` 4 + crate 8 + verify 步骤 7 dogfooding）；`ai bootstrap` 测试包随实施交付（§5.20）。
- 域 15 吸收 `--only`/`path#n` 单请求重跑选择器（`single_request_test` 8）。

**残余缺口重估**（§6.2）：~~SSE 上浮 / capture 专项 / 并行×cookie 契约 / JSON schema / TUI stream / TUI warnings~~ 六项真缺口已全部清偿——TUI 两项 2026-10-29（tui-fix 批次），其余四项 2026-10-30（test-gaps 批次，§5.4/§5.6/§5.10/§5.18）；`--env-file` 现状覆盖未确认降 P2 待核对；`load_generator` 架构债销账（已移出示例）；rupost-ai "无引用方"表述终结（已接线）。

### v1 → v2（2026-10-25，双子代理交叉审查）

成稿后由两个独立子代理并行审查（功能完整性 / 事实准确性），全部发现及处置：

**事实修正（2 处）**
1. **基线拆分错误**：v1 记 "tests/ 151 集成"，实为 `145`（另 6 个是 `cli::parser` 单测经 `main.rs` 编入主二进制）；"27 套件"实为 26 测试二进制 + 1 空 Doc-tests 结果行。已按 317 + 6 + 145 = 468 修正口径。
2. **TUI 冒烟矩阵失实**：v1 引用 "主题×错误态叉乘矩阵"，经核查 `tests/tui_smoke_test.rs` 无主题轴、无错误态叉乘（主题仅由 `theme.rs` 单测覆盖；progress_summary 的历史表述与现状有出入）。已修正描述并将主题轴列为 P2 缺口。

**功能遗漏补齐（影响矩阵结构的 3 处 + 若干条目级）**
1. **`generate` 命令整域缺位**：`src/generator/http.rs` 与 `history/selector.rs` 0 测试、`tests/` 零命中，README 明确宣称该能力——新增域 14 合并"历史 + 生成"，列为 P0 首项。
2. **`history export` 是未实现桩**：`main.rs:70-72` 仅打印 not implemented——从"可测契约"改判为"桩行为锁定测试"，避免给桩写行为测试。
3. **3 个现存测试文件游离于矩阵外**：`end_to_end_test`(9) 归域 1、`user_agent_test`(4) 新增域 19、`logger_integration_test`(1) 归域 15。
4. 条目级补齐：`--debug`/`--debug-on-failure` 失败自动诊断（域 9）、`diagnose --report json`（域 9/18）、CLI 内联请求 Cookie 接线（域 7）、`--env-file` 静默吞错现状契约（域 3）、TUI 鼠标/流式渲染/`tui <file>` 预加载（域 16）、模板三类与 `--list`（域 17）、断言运算符 7→8 修正（域 2）。

**立场补齐**
- `crates/rupost-ai` 登记为"translator 有单测、client/error 无测试且 crate 暂无引用方"，测试策略上暂不投入（P2 备注）。（v3 注：该立场已因 AI 激活实施而终结）
- `anyhow` 残留与 `RunParams` 缺位登记为"影响测试可断言性"的架构债，重构时同步补测（P2 备注），测试战役不先行处理。

**审查确认无改动项**
CLI 别名全表与全局参数集合、批量 DAG、Cookie 三模式、Mock Trie/变体、WS 剧本与自愈参数（5 次重连/100 帧缓冲/1000 帧上限）、SSE 1MB 上限、并行 Cookie 不落盘、replay 改写与 timeout 透传、CI 三平台、benches 3 基准、run_all.sh 15 用例——均与仓库事实相符。
