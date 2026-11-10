# RuPost 项目态势与技术决策检查点 (Project Checkpoint)

> **文档定位**: 记录 RuPost 当前工程基线、系统运行态势与核心技术选型决策（SSOT）。  
> **具体事项台账**: 历史交付流水与功能台账统一收敛至 [`doc/plans/progress_summary.md`](doc/plans/progress_summary.md)；核心领域设计见 [`doc/architecture/features/`](doc/architecture/features/)。

---

## 1. 当前工程态势与质量基线 (Project Posture & Quality Baseline)

* **项目定位**: 基于文档（`.http` 与 `.md`）驱动的极简高性能 API 测试、回归与排障引擎。
* **分支与代码基线**:
  * **主干分支**: `main`（加固战役分支 `feature/decoupling-and-workbench-hardening` 已通过 **PR #13** 完整合流）。
  * **当前活跃开发工作区**: `.workspace/cross-scenario-plugins`（全场景自适应与微内核插件生态专项工作区）。
* **质量保障金字塔**:
  * **单元测试**: 590+ 个核心库单测（`cargo test -p rupost --lib`）**100% PASS**（0 失败、0 忽略）。
  * **集成冒烟测试**: `tests/verify_features.sh` 涵盖 14 个核心特性（脱敏扩展性、Mock 变体、网络诊断瀑布条、DAG 拓扑、AI 机械降级、cURL 导入等）全部通过。
  * **真实端到端回归**: `examples/run_all.sh` 涵盖 15 套真实复杂场景（变量级联、SSE 流式、WebSocket 握手）全绿。
  * **Web 前端单元测试**: `tests/web/app_test.mjs` 基于原生 `node --test` + Micro-DOM 打桩（零 npm 依赖）5 测试全绿。
  * **代码质量与工具链**: Rust MSRV 1.89，`cargo clippy --workspace --all-targets -- -D warnings` 零告警，`cargo fmt` 零格式漂移。
  * **近期重大交付**:
    * `核心引擎保真与快照黑匣子演进 (Engine Fidelity & Snapshot Schema v2)`：请求体零分配嗅探（`IgnoredAny`，根除加签字典序洗牌）；快照全量强类型网络错误录制（`ExecutionErrorDetail`，Schema v2 平滑兼容）。
    * `全链路测试基建闭环 (Testing Baseline & Guard Suites)`：`ResponseFormatter` Fluent Builder 表驱动单测、TUI 关键动作与守卫直测（`tests/tui_key_actions_test.rs` + `TestBackend`）、Web 原生前端单测。
    * `凭据脱敏扩展性与合规治理 (Redaction Extensibility & Compliance Guard)`：`RedactionPolicy`、R3硬键护栏、TOML级联装配、`@sensitive` / `@no-redact` 指令、stderr审计清单Footer、变量名安全清洗。
    * `脱敏引擎同构复用与快照回放对称语义纠偏 (Redaction Reuse & Replay Parity)`：提炼 `redact_key_value_pairs` 统一同构引擎（彻底解决 URL Fragment 键值脱敏并杜绝锚点误杀）；修正快照网络错误对称回放比对逻辑；快照异常信息深度脱敏（含 `BARE_AUTH` 正则覆盖孤立 Token）；代码生成接线脱敏策略及配置加载 DRY 统一。
    * `分支深度审查加固与核心缺陷闭环 (Branch Review Hardening & Core Defect Fixes)`：
      * **P0-1 复合 URL 凭据明文泄露封堵**：重构 `variabilize_url` 与 `variabilize_query_pairs`，废除互斥分支，引入正交解构模型（主体 Query 与 Hash SPA Query 双侧独立脱敏并重组）；
      * **P0-2 `--no-history` 全局分派顶层下沉**：在 `dispatch` 最外层统一禁用录制，消除 TUI/Web/Inline 旁路泄露风险；
      * **P1-4 诊断 TLS 阶段数据保留**：`probe_after_connect` 引入 `lenient` 参数，在 HTTP TTFB 超时或 0 字节 EOF 时完整保留 TLS 耗时与证书实体；
      * **P1-5 重放引擎 `--target` 路径前缀支持**：复用 `join_paths` 支持含上下文路径的目标 URL 重放；
      * **DRY 协议规范化治理**：`UrlTemplatizer` 统一管理 WebSocket URL 规范化（RFC 6455 剥离 `#fragment`）与 origin 模板化。

---

## 2. 核心技术选型与架构决策 (Technical Choices & Architecture SSOT)

### 2.1 架构分层与领域模型 (Clean Architecture & Rich Domain Model)
* **微内核架构 (Micro-Kernel + Adapters)**:
  Request/Response 执行生命周期高度内聚于核心层；CLI、TUI、Web 与命令导入/导出严格作为外层接口适配器 (Interface Adapters)，主入口 `main.rs` 严格限制在 150 行以内，禁止命令行分支内嵌业务逻辑。
* **充血领域模型 (Rich Domain Model)**:
  以 `ParsedRequest` 实体为核心，内聚请求头查询、流式判定（`is_sse_stream`）、内容协商（`is_json_content`）等业务语义方法，严禁外部闭包解构内部字段。

### 2.2 异步并发与调度引擎 (Async Runtime & Scheduling)
* **Tokio 异步底座**: 统一调度并发网络 I/O、连接池复用、长连接状态机及异步文件限额写盘（`write_log_with_limit`）。
* **DAG 拓扑调度**: 基于 Kahn 算法实现多用例依赖解析，支持顺序与并行双模执行；依赖链状态单向克隆传递，独立分支自动实施沙箱并发会话隔离。
* **单请求精确选择器**: 原生支持 `#3` 与 `--only` 选择器语法，实现指定请求的原位重跑与断裂回路修复。

### 2.3 业务知识单一事实来源 (DRY SSOT)
系统核心知识在全库有且仅保留一个权威定义点，杜绝重复字面量与离散判定：
* **跨平台 POSIX 路径归一化**: 业务层暴露至外部（Web API、TUI、Report、快照）的所有相对路径必须经由 `src/utils/path_guard::normalize_posix_path` 纯函数收敛，拒绝反斜杠逃逸，彻底消灭 Windows/Unix 斜杠断裂。
* **协议与标准解析零堆分配**:
  * HTTP 方法通过 `Method::is_standard` 进行纯 ASCII 比较，消除堆内存分配；
  * RFC 3986 URL Scheme 前缀统一由 `has_scheme_prefix` / `has_http_scheme` 嗅探；
  * RFC 6761 本地环回地址通过 `is_loopback_host` 支持 IPv4/IPv6 与尾随点解析；
  * 多字节安全截断统一收敛至 `utils::text::{truncate_chars, truncate_utf8_bytes}`。

### 2.4 三模交互形态与工程美学 (Tri-Modal Interfaces)
* **CLI (命令行模式)**: Clap v4 传参契约严格锁定（区分子命令与内联模式位置契约），提供机器可读纯净 JSON 与高雅 ANSI 彩色终端输出。
* **TUI (终端交互模式)**: 基于 Ratatui 的 M-V-VM 架构；严格遵循**纯 ASCII 工程美学（无 Emoji）**；采用流式 Word-Wrap 预折行与向心回弹光标定位；离屏 `TestBackend` 全方位无头验证。
* **Web (活文档工作台)**: 基于 Axum 0.8 的轻量级嵌入式驱动；**Studio Pro 纯 ASCII** 专业工程符号系统；CSS 变量驱动深浅双主题（Dark / Light）自适应热切换。

### 2.5 Agent 智能体协作体系
* **规则双镜像 100% 对齐**: 根目录 `AGENTS.md` 与 `.agents/AGENTS.md` 实现零 diff 字节级一致，作为平台与智能体协作的唯一权威契约。
* **技能单一事实来源**: 全仓 8 大核心技能（AI 集成、头脑风暴、CLI 美学、前端设计、微内核架构、异步模式、编排调度、TUI 设计）全部收敛于 `.agent/skills/`。
* **TDD 驱动重构法则**: 遵循 Red-Green-Refactor 三阶段闭环；守护测试必须经受反转生产代码必然变红的验证，杜绝假守护。

---

## 3. 全场景架构与微内核插件体系规划 (Cross-Scenario & Plugin Ecosystem)
* **最新决策白皮书**: [`doc/plans/2026-11-10-cross-scenario-engine-and-plugins/05-comprehensive-architecture-and-requirements-summary.md`](doc/plans/2026-11-10-cross-scenario-engine-and-plugins/05-comprehensive-architecture-and-requirements-summary.md)
* **核心架构裁决**:
  1. **三异构场景适配**: 开发环境（插件最全）、发布环境（单二进制确定性门禁）、调试环境（单二进制即拷即走，离线数据黑匣子保真）。
  2. **插件技术选型**: 确定采用 **方案 D (Unix Stdio IPC 管道协议)**。零外部 Crate 依赖，二进制不膨胀，语言绝对自由（Python/Node/Go/Shell/Rust）。
  3. **产物契约与离线深层挖掘**: 现场录制全量失败/超时/网络时序快照（Snapshot v1.0），拷回开发机由富插件离线二次挖掘。
  4. **阻断性风险防护**: 锁定 Phase 0 决策 A（请求体零拷贝保真与 `raw_body` 保护，防止签名篡改）；锁定快照模型升级（`response: Option<...>` + `error` + `waterfall`）。
  5. **统一注册表机制 (DeepSeek Harness 模式)**: 引入 `PluginRegistry` 统一管理 CLI 自定义命令、生命周期钩子与离线挖掘器，提供 `<5ms` 的 Manifest 轻量缓存。
  6. **生命周期顺序控制与友好语法**: 废除生硬底阶语法，确立 `@auth` / `@transform` / `@sign` / `@pipeline` 语义注解，绑定 `Prepare -> Transform -> Sign` 不可逆阶段流与不可变 Patch 链；通过 JSON Schema 驱动 IDE 智能补全。
  7. **CLI 自定义命令动态路由**: 采用规范命名与双模路由，直通 TTY（支持交互终端与密码输入），100% 穿透退出码，并在 `rupost --help` 优雅聚合 EXTENSIONS 清单。
  8. **AI 预留接口体系 (AI Integration SPI)**: 严守 `ai-master-roadmap.md` B0~B4 不变量（运行期零 AI），预留脱敏上下文 (`AiContextSPI`)、离线快照分诊 (`AiMinerSPI`)、引擎语法裁决 (`EngineArbiter`) 与 MCP 双向桥梁 (`McpToolBridge`)。

