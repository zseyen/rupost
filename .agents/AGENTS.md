# AGENTS.md - Rupost 智能体协作指南

本项目的智能体协作遵循 **Clean Architecture** 原则，旨在构建一个极致简洁且高度可扩展的 API 文档驱动型测试工具。

## 项目愿景

`rupost` 是一个基于文档的 API 测试工具。我们追求在细节中发现美感，通过最简约的设计实现最强大的功能。

## 智能体角色定义

为了实现项目的卓越品质，我们定义了以下核心智能体角色，每个角色对应项目特定的 `.agent/skills`：

### 1. 架构师 (Architect) - Brainstormer
*   **关联技能**: `brainstorming`, `rupost-architecture`, `ai-integration-guidelines`
*   **职责**:
    *   将最初的创意转化为详细的设计方案和规格。
    *   在实施前进行需求分析与架构评审。
    *   平衡理想设计与项目进度，遵循 MVP 准则。
*   **原则**: YAGNI (You Ain't Gonna Need It)，一次只解决一个核心问题。

### 2. Implementation Expert (Rust Async)
*   **关联技能**: `rust-async-patterns`, `rupost-architecture`
*   **职责**:
    *   负责 `rupost` 核心引擎的开发，特别是基于 Tokio 的异步模型。
    *   处理并发网络请求、Cookie 管理、文件执行流。
    *   确保代码符合 Rust 最佳实践，追求高性能与内存安全。
*   **原则**: 遵循开闭原则，确保核心逻辑易于扩展而不必修改已有代码。

### 3. UI/UX 专家 (Frontend Specialist)
*   **关联技能**: `frontend-design`, `cli-aesthetic-ux`
*   **职责**:
    *   设计并实现 `rupost` 的用户界面（如 Dashboard 或示例页面）。
    *   确保界面美观、具有高级感，避免“AI 同质化”审美。
*   **原则**: 极简主义与极致细节，通过优秀的排版、色彩与微交互提升用户体验。

### 4. 协作编排者 (Orchestrator)
*   **关联技能**: `superpower`
*   **职责**:
    *   管理不同智能体之间的协作流程。
    *   确保在每次任务开始前调用正确的技能（Skills）。
    *   维护项目的工作流、文档一致性。

## 协作流程与规则

在 `rupost` 的开发生命周期中，无论是智能体（AI）还是人类开发者，都必须严格遵守并执行 [研发流程规范](file:///Users/zsyzzx/project/rust/rupost/doc/dev/development_workflow.md)。该流程规范由以下核心环节构成：

1. **功能分析与架构设计 (Stage 0 & 1)**：所有新功能或重大变更必须先由 **架构师** 通过 `brainstorming` 流程输出 PRD 与设计文档（存放在 `doc/plans/`），并确保设计符合 **Clean Architecture** 规范。
2. **关键代码与单元测试 (Stage 2 & 3)**：在动工前先设计核心 Trait 与数据模型，单元测试必须在 `mod tests` 编写，并通过 Mock 隔离外部网络与文件 IO。
3. **具体实现与 E2E 校验 (Stage 4 & 5)**：极致简洁实现，利用 `jj` 命令进行原子化提交，并在 `tests/` 下编写真实环境的 E2E 校验。
4. **进度归档与文档更新 (Stage 6)**：将已完成功能记录到唯一的进度事实来源 [progress_summary.md](file:///Users/zsyzzx/project/rust/rupost/doc/plans/progress_summary.md) 中，并同步更新 [README.md](file:///Users/zsyzzx/project/rust/rupost/README.md) 与 `checkpoint.md`。

---

## 5. CLI 开发与命令行传参陷阱 (CLAP Parser Rule)

在使用 `clap` 解析全局命令行参数时，必须牢记其位置解析特性：
- **全局参数**（例如具备 `global = true` 的 `--debug` 或 `--debug-on-failure`）在运行命令时，**必须紧跟在主程序名之后、子命令之前**（例如：`rupost --debug test suite.http`）。
- 若将全局参数写在子命令后面（例如：`rupost test suite.http --debug`），在没有显式进行位置参数反射处理的情况下，该参数会被子命令丢弃或误判为普通的位置参数，导致选项失效。在开发新命令行测试或编写脚本时必须严格遵循该格式。

---

## 6. CLI 入口层解耦规范 (Clean Architecture)

- **职责界限**：`main.rs` 和 `cli.rs` 属于外层接口适配器 (Interface Adapters)。其唯一职责是：解析命令行选项 (CLAP)、初始化全局基础设施 (如 Ring Cryptography Provider、Logger)、以及将命令分派给具体的领域用例 (Usecases)。
- **禁止内嵌业务**：禁止在命令行解析匹配的分支（如 `Commands::Mock`、`Commands::Test` 等）中直接包含路由读取、网络监听、依赖解析及大段转换逻辑。
- **高阶控制器下沉**：所有核心的批处理测试流、Mock 服务拉起过程必须封装在库内（例如 `rupost::runner::run_test_suite`、`rupost::mock::run_server_from_file`），主入口应当做到“即调即走”，主入口行数必须控制在 150 行以内。

## 7. 重构与测试回归保障规范 (Multi-Tiered Testing Strategy)

在对 CLI 层、Runner 核心层等关键路径进行重构或新特性注入时，必须在代码提交前执行以下三层自动化校验，不可忽略任何一层：

1. **单元测试 (Unit Tests)**：
   - 必须针对提炼出的高层服务函数编写对应的边缘用例（例如传入空路径数组、不存在的配置文件等场景），验证并阻断可能的 Panic。
2. **集成冒烟测试 (Smoke Tests)**：
   - 重构后必须在本地顺利执行 `tests/verify_features.sh` 脚本，验证包含 `init` 模板保护、`mock` 变体、`diagnose` 诊断等在内的全特性集成链路完好无损。
3. **示例与全量回归 (Regression Tests)**：
   - 执行 `cargo test` 以验证现有的集成测试和单元测试套件全部 PASS。
   - 执行 `examples/run_all.sh`，确保 examples 中内置的所有复杂场景（包括变量级联覆盖、WebSocket 握手、SSE 流式落盘等）100% 成功通过，不引入任何功能倒退。
