# RuPost 技术架构审查与改进项跟踪 (Architecture Review & Improvement Tracking)

> **审查快照**: 2026-11-17 初评 → 同日复核改进项落地状态
> **方法**: 以 [directory_structure.md](directory_structure.md) 宣称的 Clean Architecture 三层为基准，模块依赖矩阵实测（grep `crate::` 引用边）+ 抽象使用核查 + 错误体系盘点；改进项跟踪以 file:line 证据逐项销案或确认
> **总体评级**: **B+** —— 抽象真实、测试扎实、并发正确；债务集中在模块粒度与错误体系稀释，无架构性腐烂
> **文档定位**: 本文档是架构审查发现的**单一事实来源（SSOT）跟踪文档**：初评结论、复核状态、行动清单随演进滚动更新，销案项保留证据链

---

## 1. 依赖矩阵实测（初评快照）

各顶层模块的 `crate::` 引用边（不含测试代码）：

```
parser      -> runner ✗   variable
assertion   -> http ✗     utils
variable    -> parser
http        -> parser     variable
middleware  -> http       variable
ws          -> assertion  parser  utils  variable
mock        -> history    parser  runner ✗  utils  variable
history     -> http       parser  runner ✗
generator   -> history
tui         -> assertion  history  http  parser  runner  variable   （表现层，方向正确）
reporter    -> http       runner  utils                       （表现层，方向正确）
utils       -> http ✗
runner      -> assertion  history  http  middleware  parser  reporter ✗  variable  ws
```

标注 ✗ 的为违规边。**总体判断**：无底层倒挂（如 http 依赖 runner 类），违规边全部是"工具函数放错层 / 重导出 shim"级别，无架构性腐烂。

### 违规边逐条处置状态（2026-11-17 复核）

| 边 | 初评位置 | 复核状态 |
|---|---|---|
| parser → runner | `scanner.rs` 引 `runner::path::is_supported_file` | ✅ **已解决**：scanner 改委托 `crate::utils::path_guard::is_live_doc`（parser/scanner.rs:4），字面量数组收敛至 `path_guard.rs:17` 的 `LIVE_DOC_EXTENSIONS` 单一定义点，SSOT 达成（符合 AGENTS.md §9）。`runner/utils/path.rs:15` 留有纯委托包装 `is_supported_file`，无平行字面量，无害 |
| history → runner | `storage.rs` 的 `save_batch_snapshot` 吃 `TestResult` | ❌ **仍在且耦合面扩大**：函数迁至 `history/snapshot_io.rs:40`，签名仍为 `&[(PathBuf, Vec<crate::runner::types::TestResult>)]`；新增 `history/incident.rs:13` 也建立引用 |
| runner → reporter | `runner/mod.rs` 重导出 shim | ✅ **已销案**（2026-11-17 第二审查）：`runner/mod.rs` 四条跨模块桶装行（history::gc / http::url / parser::scanner::DirectoryScanner / reporter::TestReporter）整批删除，5 个调用点直呼其名（commit `2c46af77`） |
| assertion → http | `extractor.rs:2` 生产代码引 `Response`（`impl AssertTarget for Response` 写在 Core 侧） | 未复核（初评 🟡 低-中：trait 抽象本身真实，建议 impl 移到 http 侧） |
| variable → parser | `resolver.rs:53,80` 操作 `&ParsedRequest` | 未复核（初评 ⚪ 可接受：变量渲染器认识请求 AST 有内在合理性） |
| utils → http | formatter 等引 http 类型 | 未复核（初评 🟡 低） |

## 2. 抽象质量

### 2.1 真实复用的活抽象（保持，初评结论）

- **`AssertTarget` trait**：断言求值器全程 `&dyn AssertTarget`，HTTP `Response` 与 `WsFrameTarget` 双实现使同一断言引擎服务双协议——Phase 2 运算符修复一次落地两个协议即其直接红利
- **`FrameMatcher` 组合**（ws/matcher.rs）：`JsonPathMatcher` / `TextContainsMatcher` / `CompositeMatcher` 组合模式，新增匹配策略零侵入
- **并行调度 watch 通道**：依赖等待发生在信号量获取之前（等待者不占并发槽位）、`TaskOutput` 单向广播、级联失败传播
- **`PayloadDecoder` trait** → ⚠️ **2026-11-17 已塌缩**：唯一实现（MsgPackDecoder），按 YAGNI 收敛为固有方法（commit 同上批）；第二个解码器（如 protobuf）出现时从 git 历史重建 trait。§2.1 其余三条（AssertTarget 双协议、FrameMatcher 组合、并行 watch 通道）经第二审查确认均为真实活抽象，保持

### 2.2 半成品 / 空转抽象（复核更新）

| 抽象 | 初评 | 复核状态 |
|---|---|---|
| `Middleware::after_response` | 空转钩子（两个实现均 `Ok(())`），但被闭枚举 `ExecutorMiddleware` 拖住 | ❌ **钩子仍在**：`middleware/cookie.rs:234`、`middleware/routing.rs:66` 均为空操作；`sse_runner.rs:205-216` 仍构造假 `handshake_response_tmp` 调用 `run_after_response`，`http_runner.rs:176-181` 每响应仍遍历 |
| 中间件闭枚举 OCP | `ExecutorMiddleware` enum 反开闭原则 | ✅ **已解决**：重构为 `middleware/mod.rs` 的 `MiddlewarePipeline`（`Arc<dyn Middleware>` + `push`/`push_arc`，BoxFuture dyn 兼容），executor.rs 全线改用（executor.rs:27,76,133,231），带顺序/短路/克隆单元测试。新增一层不再需要改 enum |
| `Runner` enum 静态分发 | 合理技术选择，但是协议扩展的硬天花板 | 仍在（executor.rs:15）。可接受，Phase 6 `RunParams` 议程不变 |
| 孤儿 crate `rupost-ai` | workspace 声明但主 crate 零依赖、零引用 | ✅ **已解决**：现为 workspace 依赖（Cargo.toml:27），由新增的 `src/ai/` 模块（bootstrap/config/explain/resolve）真实消费 |

## 3. 内聚热点（复核更新）

| 热点 | 初评 | 复核状态 |
|---|---|---|
| `tui/state.rs` 上帝对象（1033 行，文件 IO 在 Core 层 ViewModel） | P3 待办 | ✅ **已解决**：state.rs 已不存在，TUI 重构为 `state/`、`handlers/`、`ui/` 子目录 + `ports.rs`/`theme.rs`/`files.rs`/`integrations.rs`/`ws_interactive.rs` 组件化拆分 |
| `runner/` 大杂烩 | engine/protocol/utils 二级化已缓解，executor 三合一 | 未复核（Phase 6 `RunParams` 议程不变） |
| `RequestMetadata` 字段袋 | 每加协议往通用元数据塞字段 | 未复核；注意 `src/ai/`、`src/plugin/` 等新模块可能延续该模式 |
| `__default_scheme` 魔法键 | 变量存储当隐式参数通道 | ❌ **仍在且扩散**：引用点从初评 4 处增至 13 处——应改为显式 `ExecutionContext` 参数 |
| `TestResult` 万能结果 | timing/diagnose/request/warnings 逐战役累加 | 未复核 |

## 4. 错误处理架构（复核更新）

三套体系并存：`RupostError`（thiserror 主力，`suggestion()` 是好设计）、`Result<_, String>`（ws 域为主）、`anyhow`（cookie.rs 等）。

**复核数据（恶化）**：`RupostError::Other(String)` 从初评 14 处增至 **59 处**，`Result<_, String>` 从 14 处增至 **42 处**——增量主要来自新模块（plugin、web、importer、runner/selector 等）采用快捷模式。`Other(String)` 实质是 stringly-typed 错误，调用方无法编程式分支。**建议**：为高频场景定义结构化变体（如 `InvalidEnvName`、`DependencyOutsideRoots`），并在新模块准入时把关；存量逐步收敛。

## 5. 并发模型（初评结论，本项目最强部分）

- WS 会话：actor + broadcast + 有界队列（发送 128 / 广播 1024 / 历史 1000 / 待发 100）+ 指数退避；所有 `std::sync::Mutex` 不跨 `.await`
- 并行调度：`JoinSet` + `Semaphore` + 每节点 `watch` 通道；fail-fast 用 `AtomicBool`
- Cookie：跨进程文件锁 + mutex 获取顺序恒定（file → mutex），无死锁环
- 唯一提醒：`std::sync::Mutex` 在 async 上下文的安全性靠"临界区不跨 await"约定保证，无编译期保障——建议 CI 加 `clippy::await_holding_lock` 守护

## 6. 可测试性

- 网络测试全部 wiremock（符合 AGENTS.md Mock 隔离要求），断流场景以裸 TcpListener 补 wiremock 能力缺口
- 分层测试纪律真实存在：修复与回归测试同提交
- 缺口：`Client` 是具体类型非 trait，执行路径测试必须走真实网络栈（wiremock 缓解，代价是集成测试 4–8s）；测试规模从战役起点 318 增至当前约 1299 个测试属性

## 7. 构建与分发

- **无 feature flags**：axum（Mock）、ratatui（TUI）、tokio-tungstenite（WS）、新 web 模块全部无条件编译；release 二进制 14.8 MB（2026-11-17 构建）。建议 `tui`/`mock`/`ws`/`web` feature（默认全开），投入产出比最高的架构改进之一
- **依赖风险**：reqwest 默认拉 aws-lc-rs，其 C 构建在 macOS 27 SDK 下实测失败（本机以 `SDKROOT` 规避）——发布前必须确认或切 ring
- **workspace**：rupost-ai 已真实接入，孤儿问题销案

## 8. 可扩展性实测（初评结论）

| 场景 | 改动面 | 评价 |
|---|---|---|
| 加一个断言运算符 | 2–3 处，协议无关（WS 免费获得） | ✅ 优秀 |
| 加一个 capture 源 | 枚举 + 一个 match 臂 | ✅ 良好 |
| 加一种协议（gRPC） | 6+ 处，含 Runner 闭分发 | 🟠 天花板所在 |

## 9. 期间演进记录（初评 → 复核之间）

初评后代码库经历大规模演进，以下为复核时确认的新事实：

1. **新增 9 个顶层模块**：`ai`（rupost-ai 接入与引导）、`plugin`（插件系统，含秘密环境变量隔离）、`web`（Web 工作台，server/store）、`importer`（curl 导入）、`exporter`、`diagnostics`、`report`、`security`、`platform`
2. **TUI 全面重构**：上帝对象拆解为组件化目录结构（见 §3）
3. **中间件 dyn 化**：`MiddlewarePipeline` 替代闭枚举（见 §2.2）
4. **路径 SSOT 收敛**：`path_guard` 成为文档后缀与路径规范的唯一权威定义点（见 §1）
5. **高价值安全修复**：插件子进程的 `RUPOST_*` 秘密透传豁免、重放 body 掩码、history `tail()` 预分配上限（P1 系列）
6. **事件级级联基建**：`incident.rs`（事件包）、`snapshot_io.rs`（快照 IO 独立）、`suite_presenter.rs`（表现层从 cli 主分支拆出）
7. **机械清理批（2026-11-17 第二审查）**：跨模块桶装 re-export 删除（§1 runner→reporter 销案）、redact 双路径垫片删除（`exporter/redact.rs` + `ai/redactor.rs` → 12 文件 import 收敛至 `security/redact.rs`，15 项独有测试随迁）、死抽象删除（`with_observer` 空转 builder、MockCompiler lenient 生产不可达分支、`ReportGenerator`/`PayloadDecoder` 单实现 trait 塌缩）、replay `--verbose` 响应体补脱敏

## 10. 行动清单（复核后修正版，按投入产出排序）

| # | 行动 | 状态 | 修正后工作量 |
|---|---|---|---|
| 1 | 删 runner→reporter 孤儿 shim（runner/mod.rs:36，零消费者） | ✅ 销案（随桶装行整批删除，commit `2c46af77`） | — |
| 2 | 删 `after_response` 空钩子 + sse_runner 假 Response 仪式 | ❌ 待办（新抽象下更干净） | ~1 小时 |
| 3 | `save_batch_snapshot` DTO 化 | ❌ 待办，**优先级上调**（incident.rs 正在扩大耦合面） | ~半天 |
| 4 | feature flags（tui/mock/ws/web，默认全开） | ❌ 待办 | ~半天 |
| 5 | CI 加 `clippy::await_holding_lock` | ❌ 待办 | ~10 分钟 |
| 6 | `__default_scheme` 改显式 ExecutionContext 参数 | ❌ 待办（13 处引用，趁早） | ~1 小时 |
| 7 | `RupostError::Other` 结构化 + 新模块错误体系把关 | ❌ 待办（59 处，随业务演进逐步） | 持续 |
| 8 | assertion → http 边治理（`impl AssertTarget for Response` 移到 http 侧） | ❌ 待办 | ~半小时 |
| 9 | rupost-ai 孤儿 | ✅ 销案 | — |
| 10 | TUI state.rs 拆分 | ✅ 销案 | — |
| 11 | is_supported_file 归位 / SSOT | ✅ 销案 | — |
| 12 | 中间件闭枚举 OCP | ✅ 销案 | — |
| 13 | warning 四通道收敛（`TestResult.warnings` 唯一事实 / WarningSink 仅实时渲染 / 业务路径 66 处裸 `eprintln!` 清零——与 warning_sink.rs 自我声明矛盾） | ❌ 待办（2026-11-17 第二审查新增，证据：runner/executor_plugin.rs 双写、diagnostics/warning_sink.rs:1-3） | ~1 天 |
| 14 | report 双管线合并（`from_file_results` 在线直渲与 `from_json_file_reports` 离线逐字段平行——JUnit 已走 JsonFileReport 单路径，照抄即可） | ❌ 待办（第二审查新增，证据：report/types.rs:191 vs 316-428） | ~半天 |
| 16 | 新代码函数族收敛：`parse_plugin_env_spec/_specs/_with_warnings` 三函数族（batch 内联复制未委托、Option 版生产已死）；诊断域 6 入口 + strict/lenient 双体（`connect_tcp_with_timeout` 仅剩 1 生产调用者） | ❌ 待办（2026-11-18 第三审查·新鲜眼盲扫新增，证据：plugin/pipeline.rs:42-101、http/diagnose.rs:371/438） | ~半天 |
| 17 | ConfigLoader 16 pub fn：同一 rupost.toml 三条加载路径三种损坏策略（redaction→builtin / plugins→default / variables→None+warning），新人无从选择 | ❌ 待办（同上，证据：variable/config.rs:88/140/165） | 收口 ~1 天 |
| 18 | mock 插件测试 DSL 协议零文档：未知指令静默跳过（`_ => {}`）、stdin 单次消耗、分段数不一（split_once/splitn(4)/splitn(3)）——8 个测试文件共用，新测试已三踩 | ❌ 待办（同上，证据：tests/common/mock_plugin.rs:172 起；三踩记录见 plugin_pipeline/plugin_dod 测试注释） | fail-loud + 指令表 ~2h |
| 15 | mock 生命周期三写归口（web/mock_manager.rs 576 行 × tui/integrations.rs 325 行各自实现启停/端口/流量环 → 抽宿主无关 `mock/session.rs`） | ❌ 待办（第二审查新增） | ~1 天 |

> **维护约定**：每完成一项，在本表更新状态并保留销案证据（file:line）；新发现按 §1–§8 归类后入表，禁止口头跟踪。
> **交叉引用**：2026-11-17 第二次接手成本审查（命名/上手路径/性能三路）的完整报告在会话交付中；其架构级发现已按本约定并入 §10（#13–#15），性能类（TUI 合帧/插件 spawn 串行/CI 平台期望表）与文档治理类不在本表范围，见各自台账。
