# RuPost 全量测试方案 (Test Strategy & Plan)

> **创建日期**: 2026-10-25（v2，经两个独立子代理——功能完整性审查 / 事实准确性核查——交叉审查后修订，修订记录见 §9）
> **性质**: 全局测试策略与功能覆盖矩阵（唯一测试方案事实来源，领域专项规约如 [Cookie 测试规约](file:///Users/zsyzzx/project/rust/rupost/doc/cookie/cookie_test_specification.md) 以本方案为父纲）
> **现状基线**: 全量 468 测试 0 失败 = src `mod tests` 317 单测 + 主二进制内嵌单测 6（`cli::parser` 经 `main.rs` 编入）+ `tests/` 145 集成测试；`cargo fmt --check` 与 `cargo clippy --all-targets -D warnings` 双零
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
| **L1** | 各源文件 `mod tests`（`#[cfg(test)]`，56 个模块） | 内存 Mock（如 `MockHttpClient`）、`InMemory FileRepository` | 317 个（lib）+ 6 个（主二进制 `cli::parser`） | `cargo test`，每次提交 |
| **L2** | `tests/*.rs`（23 个文件） | wiremock（11 文件）、裸 `TcpListener`（4 文件）、`MockLlmServer`、`TempDir`、ratatui `TestBackend` | 145 个 | `cargo test`，每次提交 |
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

**外网依赖收敛规则**：自动化测试（L0–L4）**禁止**发起真实公网请求；仅 L5 手工清单允许，且统一收敛到 `httpbingo.org`（功能）与 `badssl.com`（证书异常）。

---

## 4. 功能域覆盖矩阵（19 域 × 现状 × 缺口）

> 状态标记：✅ 覆盖充分 ｜ ⚠️ 覆盖偏薄（有明确缺口） ｜ 🕳 覆盖盲区（见 §6 行动计划）

| # | 功能域 | 核心行为契约 | 现有测试资产 | 状态 |
| :-- | :--- | :--- | :--- | :--- |
| 1 | 文件解析（`.http`/`.md`） | 语法围栏提取、`###` 切分、启发式漏写分隔符自适应、注释前缀剥离、info string | `markdown_integration_test`(8)、`end_to_end_test`(9)、src 内 parser 单测 | ✅ |
| 2 | 断言系统 | 8 运算符矩阵（`== != contains > >= < <= exists`）、None/null 语义、统一 JSONPath、f64 精确相等、引号感知最左扫描 | `adversarial_regression_test`(7)、src 断言单测 | ✅ |
| 3 | 变量与环境 | 四层级联优先级（`--var` > 系统 env > `.env` > `rupost.toml`）、动态变量、未定义警告、`--env` 路径穿越拒绝、`--env-file` 无效路径静默忽略（现状契约） | `variable_integration_test`(13) | ✅ |
| 4 | 变量捕获 | `@capture` 逐条化、失败进 warnings、`global.` 跨文件传递、regex/JSONPath 源 | `variable_integration_test`、批量用例级联 | ⚠️ 缺独立域测试（捕获行为散落在变量/批量测试中） |
| 5 | 批量测试与 DAG | Kahn 拓扑、环路检测、依赖补全、路径合理性校验、State Cloning、Fail-Fast | `batch_testing_test`(9) | ✅ |
| 6 | 并行与 Cookie 隔离 | JoinSet 并发限制、并行分支 Cookie 物理隔离（不落盘）、钻石 DAG 告警 | `cookie_isolation_test`(2)、`batch_testing_test` | ⚠️ 隔离矩阵待补（并行 × `--cookie-file` 组合契约未锁定） |
| 7 | Cookie 管理 | 三模式（AutoPersist/Ephemeral/Disabled）、文件锁、0600、损坏降级、CLI 内联请求的 `--no-cookies`/`--cookie-file`/`--env`→`cookies_<env>.json` 接线 | `cookie_test`(9) + [Cookie 测试规约](file:///Users/zsyzzx/project/rust/rupost/doc/cookie/cookie_test_specification.md) | ✅（CLI 内联接线仅有函数级测试） |
| 8 | 本地 Mock 服务 | Trie 路由（`:id`/`*`/`**`）、多变体条件、兜底分支、Axum 适配 | `mock_integration_test`(3) + src mock 单测 | ✅（单测充分） |
| 9 | 网络诊断 | DNS/TCP/TLS/TTFB 拆分、X.509 临期/过期判定、`--debug`/`--debug-on-failure` 失败自动诊断、`diagnose --report json` 输出 | `diagnose_integration_test`(3)、`timing_diagnostics_test`(2) | ⚠️ 失败自动诊断与 JSON 输出无测试 |
| 10 | SSE 流式 | WHATWG 语义（空 data 帧、event 重置、id 留存）、内存上限、`@sse_timeout`、`@stream_to` | `sse_assertion_test`(2) + src SseRunner 单测 | ⚠️ 集成层偏薄，断流/上限场景需上浮 |
| 11 | LLM 流式调试 | OpenAI/Anthropic 帧规整、`stream.llm.content` 断言、路由重定向 | `llm_mvp_test`(5) | ✅（`crates/rupost-ai` translator 有 mockall 单测，client/error 无测试且 crate 暂无引用方，见 §6 P2） |
| 12 | WebSocket | `@websocket` 剧本（SEND/EXPECT/WAIT/CLOSE）、软匹配、MsgPack、重连自愈、1000 帧上限 | `websocket_integration_test`(8) + src ws 单测 | ✅ |
| 13 | 快照录制与重放 | `--save-snapshot` 录制、`replay --target` 改写、`@timeout` 透传 | `snapshot_replay_test`(1) | 🕳 单测试承载整个域 |
| 14 | 请求历史与测试文件生成 | `history list/show/prune/clear` 四实子命令（`export` 为 MVP 桩，行为=打印未实现提示）、gc、并发写；`generate`（别名 `g`，`--last`/`--interactive`）从历史生成 `.http` | `history_integration_test`(1)、`history_concurrency_test`(1) | 🕳 `generator/http.rs` 0 测试（selector 的 Last 策略已有 2 单测，Interactive 为 TTY 交互归 L5）；历史子命令仅 2 测试 |
| 15 | CLI 体验 | curl/httpie 解析、单字母别名（t/h/g/d/m/i/r/l/e/s/p/c）、**全局参数必须置于子命令前**、URL 归一化、日志级别（`RUST_LOG` 过滤与 debug/release 默认级） | `query_params_test`(3)、`auto_routing_test`(2)、`logger_integration_test`(1)、src cli/parser 单测 | ⚠️ 全局参数位置陷阱无回归锁定 |
| 16 | TUI 交互 | MVU 状态机、双 Tab、预折行滚动、`/` 过滤、快速新建三防线、鼠标点击切焦、流式帧增量渲染、`tui <file>` 预加载入口、主题切换（单测覆盖） | `tui_smoke_test`(39) + src 纯函数单测 | ⚠️ 流式渲染（`state/stream.rs`）0 测试；主题轴未进冒烟矩阵；`warnings` 渲染盲区（遗留项 D1） |
| 17 | 模板脚手架 | `init` 三类模板（config/sse/llm）生成、`--list`、防覆盖、`--force` 警告、模板内容完整性、`template` 别名 | `template_test`(9) + L3 脚本 | ✅ |
| 18 | 报告与错误处理 | 终端彩色摘要、JSON 报告 schema（含 warnings）、panic 清零（用户输入→失败结果非崩溃） | `error_test`(4)、JSON 报告断言散落 | ⚠️ JSON schema 契约无独立锁定 |
| 19 | HTTP 客户端行为 | 默认 UA `rupost/1.0.0`、变量配置覆盖、用例 Header 最高优先级覆盖的三级级联 | `user_agent_test`(4) | ✅ |

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
- **现状契约锁定**：`--env-file` 指向无效路径时当前为**静默忽略**（`config.rs` 解析失败被 `.ok()` 吞掉，质量加固方案已登记为债）——以测试锁定现状；未来若改为告警，测试先行失败提示同步更新。

### 5.4 变量捕获（缺口域）
- **待补**：`@capture` 失败逐条降级（一坏一好 → 好的生效、坏的进 `warnings`）、`from_id from body.id` 变量名含 `from` 的解析、regex 源内含 `from` 子串、`global.` 前缀跨文件克隆后的可见性。目标落点：新增 `tests/capture_integration_test.rs` 或并入 `variable_integration_test.rs`。

### 5.5 批量测试与 DAG
- **拓扑场景**：线性链、Fork/Join、钻石（多父）、成环（报 `Dependency loop detected` 后中断）、缺失依赖自动补全、`@depends-on ../../` 越界拒绝。
- **并行专项**：`--concurrency` 上限生效、Fail-Fast 在并行模式的中断语义、State Cloning 单向性（子节点写变量不回渗父节点）。

### 5.6 并行 × Cookie 组合（缺口域）
- **待补**：并行模式下 `--cookie-file` 的行为契约（当前实现为串行分支才落盘，需以测试锁定"并行不落盘"为显式文档化行为，防止未来无感变更）。

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
- **缺口**：断流 flush 与内存上限目前集中在 src 单测，需各上浮 1 个集成测试。

### 5.11 LLM 流式
OpenAI/Anthropic 双格式的 delta/content_block 帧规整为 `stream.llm.content`；非 LLM 流降级 Generic；`MockLlmServer` 提供本地闭环；路由中间件把外部 host 改写为本地 Mock。

### 5.12 WebSocket
- **剧本矩阵**：SEND→EXPECT 软匹配（高频广播干扰下不雪崩）、WAIT 超时、CLOSE、MsgPack 二进制解码后断言。
- **自愈**：物理断连指数退避 5 次重连、挂起期 100 帧缓冲、重连后 `push_front` 时序补发。
- **资源**：`BoundedFrameBuffer` 1000 帧上限滑动淘汰。

### 5.13 快照录制与重放（缺口域）
- **待补**（当前仅 1 个集成测试）：
  1. 录制 → 重放往返等价：`--save-snapshot` 后 `replay --target host:port`，断言 Path/Query 原样、Host/Port 改写生效；
  2. `@timeout` 语义透传（快照含 timeout 字段的重放不再退回 30s 默认）；
  3. 快照文件损坏/非快照 JSON 的可读错误。

### 5.14 请求历史与测试文件生成（缺口域）
- **历史子命令**（四个实子命令仅 2 测试，待补）：`history list --limit`、`show <id>`、`prune`/`clear` 的删除语义与 gc 触发；并发写不损坏（已有 1 测试守护）。
- **`export` 为 MVP 桩**：`main.rs` 对 `HistoryCommands::Export` 仅打印 "not implemented in MVP stage." 并丢弃参数——以 1 个**桩行为锁定测试**记录现状（输出包含该提示、退出码 0），防止有人误以为它已可用；实现 export 时该测试先行失败驱动替换。
- **`generate` 域（`generator/http.rs` 0 测试，最高缺口；selector Last 策略已有 2 单测，Interactive 为 TTY 交互归 L5 手工）**：
  1. `generate --last` 非交互路径：最近一次请求快照 → 生成合法 `.http` 文件（文件存在、含方法行与 URL、可被 `rupost t` 再次解析执行——往返闭环）；
  2. 交互选择路径（`history/selector.rs`）：以预置输入驱动选择逻辑（优先纯函数层测试）；
  3. 空历史 / 历史为空目录的友好错误。
- **TUI 反向还原**：`format_request_snapshot_to_http` 纯函数已单测覆盖，此处不重复。

### 5.15 CLI 体验（待锁回归）
- **全局参数位置契约**（AGENTS.md CLAP 规则，2026-10-25 实证校准）：子命令模式下 clap v4 对 `global = true` 参数**前后位置皆可**（实测 `rupost test x.http --debug` 正确解析）；**真实陷阱在内联模式**（无子命令）——后置全局旗被 `trailing_var_arg` 吞入内联参数且改变解析器选择。以表驱动解析级测试锁定双模式八场景，并同步修正 AGENTS.md 表述（详见 [P0 执行方案 §5](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-25-test-p0-execution.md)）。
- **解析器**：curl 风格（`-H`/`-d`/`-X`）、httpie 风格、`strict json` 错误、单字母别名全表（t/h/g/d/m/i/r/l/e/s/p/c）冒烟。
- **URL 归一化**：`normalize_input_url` 决策表（`:3000` 缩写、无 scheme 补全、WS/WSS 原样返回、query 含 `://` 不误判）。
- **日志级别**（`logger_integration_test` 守护）：`RUST_LOG` 过滤生效、debug/release 构建的默认级别差异不泄漏调试噪音。

### 5.16 TUI
- **冒烟矩阵**（TestBackend，39 个平铺场景）：布局三态（Wide/Narrow/Stacked）× Tab（Files/History）× 面板切焦 × 固定终端尺寸。**主题轴未进冒烟矩阵**（主题切换仅由 `src/tui/theme.rs` 单测覆盖明暗双调色板取值）——登记为待补维度。
- **纯函数层**：`nav` 决策表、`relocate_selection` 表格测试、`filter_matches` 谓词、`log_decision` 决策表、预折行中英文/Emoji 切分、鼠标 `hit_test` 纯几何。
- **安全三防线**：路径穿越拒绝、`create_exclusive` 原子创建、超长文件名截断+哈希（unix 门控）。
- **待补**：`state/stream.rs` 流式帧增量预折行渲染（**0 测试**，SSE/WS 日志在 TUI 中实时展示的核心路径）；`rupost tui <file>` 文件预加载入口；`TestResult.warnings` 渲染（遗留项 D1，功能补齐后补冒烟）。

### 5.17 模板脚手架
空目录初始化、重名阻断报错、`--force` 高亮警告不中断、三类模板（config/sse/llm）生成与 `--list` 枚举、模板内容完整性（`environments.dev` 存在性校验）、`template` 别名——L2（9 测试）与 L3 脚本双层守护。

### 5.18 报告与错误处理
- **JSON 报告 schema 契约**：字段清单锁定（含 `warnings`），新增字段必须同步更新 schema 断言——防止"手写字段清单漏字段"缺陷复发（质量加固 C9 锚点）。
- **panic 清零纪律**：任何用户输入（状态码、断言、header 名）只允许产生失败的测试结果；每个 panic 修复与 wiremock/TcpListener 复现用例同提交。

### 5.19 HTTP 客户端行为
`user_agent_test` 三级级联：默认 UA `rupost/1.0.0` → 变量配置覆盖 → 用例 `User-Agent` Header 最高优先级覆盖；随域 15 的内联请求冒烟互为补充。

---

## 6. 缺口行动计划（Gap → 优先级）

> 按"每项可独立交付、可砍"原则列项，遵循 MVP 准则。**P0 四项的可执行落地细化（含逐场景 Given-When-Then 说明）见 [P0 执行方案](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-25-test-p0-execution.md)**。

| 优先级 | 缺口项 | 动作 | 验收标准 |
| :--- | :--- | :--- | :--- |
| **P0** | `generate` 域：`generator/http.rs` 0 测试（§5.14） | 新增 generator 测试：`--last` 生成→可再解析的往返闭环、空历史友好错误 | `tests/` 含 generate 往返闭环测试 |
| **P0** | 快照录制/重放域仅 1 集成测试（§5.13） | 新增 3 个集成测试（往返等价/timeout 透传/损坏容错） | `tests/snapshot_replay_test.rs` ≥ 4 测试全绿 |
| **P0** | 历史四实子命令仅 2 集成测试 + `export` 桩未锁定（§5.14） | 补 `list --limit`/`show`/`prune` 行为测试 + `export` 桩行为锁定测试 | 四实子命令各有 ≥1 集成测试；桩行为有锚点 |
| **P0** | CLI 全局参数位置契约无回归锁定（§5.15） | 新增 2 个二进制级测试锁定全局参数前置/后置行为 | `tests/` 内有显式命名测试；行为变更时测试先行失败 |
| **P1** | SSE 断流 flush / 1MB 上限未上浮集成层（§5.10） | 各上浮 1 个 TcpListener 集成测试 | 集成层可独立复现两个语义 |
| **P1** | 捕获域无独立测试文件（§5.4） | 新增 `capture` 专项测试（逐条降级/`from` 解析/global 克隆） | §5.4 清单全覆盖 |
| **P1** | 并行 × `--cookie-file` 行为契约未锁定（§5.6） | 新增 1 测试锁定"并行分支不落盘"契约 | 契约变更时测试先行失败 |
| **P1** | JSON 报告 schema 无独立契约测试（§5.18） | 新增 schema 断言（字段清单逐项存在性），含 `diagnose --report json` | 手写字段清单漏字段时测试失败 |
| **P1** | `--debug-on-failure` 失败自动诊断无测试（§5.9） | 新增 1 个失败用例触发的自动诊断集成测试 | 失败输出含时延拆分段 |
| **P1** | TUI 流式渲染（`state/stream.rs`）0 测试 + `tui <file>` 预加载（§5.16） | 补流式增量渲染单测 + 预加载入口冒烟 | §5.16 待补清单清零 |
| **P1** | TUI `warnings` 渲染盲区（D1 遗留） | 功能补齐后补 1 冒烟测试 | 冒烟含含 warning 的结果态 |
| **P1** | `--env-file` 无效路径静默忽略未锁定（§5.3） | 新增 1 测试锁定现状契约 | 契约变更时测试先行失败 |
| **P2** | 覆盖率统计未常态化（tarpaulin 仅文档提及） | 本地可选命令固化为 `make`/脚本入口；暂不进 CI 门禁（避免 flaky 外网依赖） | [测试指引 §4.2](file:///Users/zsyzzx/project/rust/rupost/doc/guides/testing_guide.md) 命令可一键执行 |
| **P2** | criterion 基准无趋势观测 | 发版前手动跑 3 基准比对；`load_generator` 移出 `[[bin]]`（架构债顺带） | benches 可运行且耗时无数量级回退 |
| **P2** | 主题轴未进 TUI 冒烟矩阵（§5.16） | 在现有 TestBackend 冒烟上加 1 个主题维度参数化 | Dark/Light 各 1 渲染冒烟 |
| **P2** | 架构债对测试策略的影响登记 | `anyhow` 残留（`middleware/cookie.rs`、`utils/formatter.rs`、`error.rs` From）限制错误路径类型化断言；`RunParams` 参数对象尚不存在影响可参数化；`crates/rupost-ai` client/error 无测试且 crate 无引用方 | 重构执行时同步按本方案补测试，不在测试战役中先行处理 |
| **P2** | Windows 平台文件锁/权限差异 | 现有 `#[cfg(unix)]` 门控维持；CI 三平台绿即达标 | CI test Job 三平台 0 失败 |

**P0 合计约 10 个新测试，预计 1–2 天内可完成；P1 合计约 8 个新测试。**

---

## 7. 执行纪律与成功指标

### 7.1 强制纪律（继承研发流程规范）
1. **提交前三步**：`cargo fmt --all -- --check` → `cargo clippy --all-targets --all-features -- -D warnings` → `cargo test` 全绿，缺一不提交。
2. **核心引擎改动**（parser / executor / batch / parallel）：追加跑 `examples/run_all.sh` 与 `tests/verify_features.sh`。
3. **Bug 修复 = 修复 + 对抗性回归测试同一原子提交**（`jj`），杜绝"修了但没锁"。
4. **新增功能 = 功能 + 对应层级测试同一原子提交**，且在 §4 矩阵中登记状态。

### 7.2 成功指标（本方案的生命周期）
- **基线守护**：全量测试 0 失败、fmt/clippy 双零、`run_all.sh` 15/15、`verify_features.sh` PASS。
- **矩阵收敛**：§4 中 🕳 清零、⚠️ 全部降级为 ✅ 或明确接受为文档化契约（P1 完成后达成）。
- **追溯性**：任何测试可沿"用户故事 → 行为契约 → 测试文件"链条追溯；任何 bug 修复可在 `adversarial_regression_test.rs` 或对应域测试中找到锚点。

---

## 8. 文档联动

- 本方案为测试侧唯一父纲；[测试与功能验证指引](file:///Users/zsyzzx/project/rust/rupost/doc/guides/testing_guide.md) 保留为"E2E 手工操作手册"并在开头反向链接本方案。
- 领域专项规约（如 [Cookie 测试规约](file:///Users/zsyzzx/project/rust/rupost/doc/cookie/cookie_test_specification.md)）从属于本方案 §4 对应行。
- 进度事实唯一来源仍为 [progress_summary.md](file:///Users/zsyzzx/project/rust/rupost/doc/plans/progress_summary.md)，本方案执行进展按 Stage 6 规范登记其中。

---

## 9. 子代理交叉审查修订记录（v1 → v2）

成稿后由两个独立子代理并行审查（功能完整性 / 事实准确性），全部发现及处置：

### 事实修正（2 处）
1. **基线拆分错误**：v1 记 "tests/ 151 集成"，实为 `145`（另 6 个是 `cli::parser` 单测经 `main.rs` 编入主二进制）；"27 套件"实为 26 测试二进制 + 1 空 Doc-tests 结果行。已按 317 + 6 + 145 = 468 修正口径。
2. **TUI 冒烟矩阵失实**：v1 引用 "主题×错误态叉乘矩阵"，经核查 `tests/tui_smoke_test.rs` 无主题轴、无错误态叉乘（主题仅由 `theme.rs` 单测覆盖；progress_summary 的历史表述与现状有出入）。已修正描述并将主题轴列为 P2 缺口。

### 功能遗漏补齐（影响矩阵结构的 3 处 + 若干条目级）
1. **`generate` 命令整域缺位**：`src/generator/http.rs` 与 `history/selector.rs` 0 测试、`tests/` 零命中，README 明确宣称该能力——新增域 14 合并"历史 + 生成"，列为 P0 首项。
2. **`history export` 是未实现桩**：`main.rs:70-72` 仅打印 not implemented——从"可测契约"改判为"桩行为锁定测试"，避免给桩写行为测试。
3. **3 个现存测试文件游离于矩阵外**：`end_to_end_test`(9) 归域 1、`user_agent_test`(4) 新增域 19、`logger_integration_test`(1) 归域 15。
4. 条目级补齐：`--debug`/`--debug-on-failure` 失败自动诊断（域 9）、`diagnose --report json`（域 9/18）、CLI 内联请求 Cookie 接线（域 7）、`--env-file` 静默吞错现状契约（域 3）、TUI 鼠标/流式渲染/`tui <file>` 预加载（域 16）、模板三类与 `--list`（域 17）、断言运算符 7→8 修正（域 2）。

### 立场补齐
- `crates/rupost-ai` 登记为"translator 有单测、client/error 无测试且 crate 暂无引用方"，测试策略上暂不投入（P2 备注）。
- `anyhow` 残留与 `RunParams` 缺位登记为"影响测试可断言性"的架构债，重构时同步补测（P2 备注），测试战役不先行处理。

### 审查确认无改动项
CLI 别名全表与全局参数集合、批量 DAG、Cookie 三模式、Mock Trie/变体、WS 剧本与自愈参数（5 次重连/100 帧缓冲/1000 帧上限）、SSE 1MB 上限、并行 Cookie 不落盘、replay 改写与 timeout 透传、CI 三平台、benches 3 基准、run_all.sh 15 用例——均与仓库事实相符。
