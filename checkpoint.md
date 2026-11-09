# RuPost 项目态势与技术决策检查点 (Project Checkpoint)

> **文档定位**: 记录 RuPost 当前工程基线、系统运行态势与核心技术选型决策（SSOT）。  
> **具体事项台账**: 历史交付流水与功能台账统一收敛至 [`doc/plans/progress_summary.md`](doc/plans/progress_summary.md)；核心领域设计见 [`doc/architecture/features/`](doc/architecture/features/)。

---

## 1. 当前工程态势与质量基线 (Project Posture & Quality Baseline)

* **项目定位**: 基于文档（`.http` 与 `.md`）驱动的极简高性能 API 测试、回归与排障引擎。
* **分支与代码基线**:
  * **主干分支**: `main`（加固战役分支 `feature/decoupling-and-workbench-hardening` 已通过 **PR #13** 完整合流）。
  * **工作区收敛**: 仅保留单一 `default` 主工作区，所有历史/辅助工作区已全部注销并出清，工作副本保持 clean。
* **质量保障金字塔**:
  * **单元测试**: 564 个核心库单测（`cargo test -p rupost --lib`）**100% PASS**（0 失败、0 忽略）。
  * **集成冒烟测试**: `tests/verify_features.sh` 涵盖 Mock 变体、网络诊断瀑布条、DAG 拓扑、AI 机械降级、cURL 导入等全部通过。
  * **真实端到端回归**: `examples/run_all.sh` 涵盖 15 套真实复杂场景（变量级联、SSE 流式、WebSocket 握手）全绿。
  * **代码质量与工具链**: Rust MSRV 1.89，`cargo clippy --all-targets -- -D warnings` 零告警，`cargo fmt` 零格式漂移。

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
