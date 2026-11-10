# RuPost 演进规划与方案总纲 (Evolution Plans & PRD Nexus)

本目录承载 RuPost 从诞生至今的所有产品需求文档 (PRD)、技术实施方案、架构演进计划与版本路线图。
为了消解历史迭代中 50+ 篇规划文档的重复表述与版本碎片，本项目建立了统一的**演进主线与交叉引用矩阵**，实现所有历史决策 100% 具备可追溯性，同时长青追踪唯一聚焦。

---

## 1. 核心长青规划事实来源 (Single Source of Truth)

在日常开发与迭代中，所有开发者与智能体仅需聚焦以下两个唯一事实来源文件：

*   🎯 **[progress_summary.md](progress_summary.md)**：**项目唯一的当前开发进度事实来源**。记录已完成阶段、在仓代码映射与测试用例验证。
*   📋 **[future_backlog.md](future_backlog.md)**：**统一未来待办需求池**。记录已收录但暂未排期的技术债务与前瞻特性（P1 / P2 / P3 / Won't Do）。
*   🧭 **[rupost_product_strategy.md](rupost_product_strategy.md)**：**产品战略与行业差异化定位**。确立“面向实时 AI 时代的终端回归引擎”核心护城河。

---

## 2. 演进主线内容深度汇总与交叉引用 (Evolution Threads)

历史上的 50 余篇 Plan 围绕 5 条核心业务主线演进。我们已将其重复与冲突内容深度消化，提炼为统一的**权威架构档案**。各条主线的演变全貌与历史文档溯源如下：

### 2.1 主线一：AI 智能赋能演进线 (AI Intelligence Thread)
*   **现行统一架构档案**：[`doc/architecture/features/ai_engine_system.md`](../architecture/features/ai_engine_system.md)
*   **演变全貌**：
    早期可行性评估（Scheme A vs Scheme B 权衡）→ 启动激活方案 (`ai-activation-prd`) → 自然语言转断言设计 (`ai-context-assert`) → 失败批量归因与分诊 (`ai-failure-triage`) → 独立 Crate 接线与 E2E 交付 (`ai-e2e5-master`)。
*   **涵盖的历史方案追溯矩阵**：
    | 历史方案文件名 | 对应阶段 | 核心贡献与演进结果 | 吸收落地位置 |
    | :--- | :--- | :--- | :--- |
    | `2026-10-26-ai-activation-prd.md` | 激活战役 | 明确激活死代码 `crates/rupost-ai` 的 PRD 规格 | `crates/rupost-ai/` |
    | `2026-10-26-ai-bootstrap-proposal.md` | 技术方案 | 确立子 Crate 嵌入式编译架构与配置模型 | `crates/rupost-ai/Cargo.toml` |
    | `2026-10-27-ai-context-assert-and-zero-config-prd.md` | 语法设计 | 提出自然语言零配置断言翻译算法 | `src/translator.rs` |
    | `2026-10-27-ai-context-assert-test-plan.md` | 测试保障 | 断言翻译测试用例集与 Mock 策略 | `tests/ai_integration_test.rs` |
    | `2026-10-27-ai-developer-journey-design.md` | 用户旅程 | 规范从编写到失败修复的完整 DX 体验 | CLI `rupost ai` 入口 |
    | `2026-10-27-ai-master-roadmap.md` | 路线规划 | AI 演进八大里程碑 (F1-F8) 全貌 | 路线图规划池 |
    | `2026-10-27-ai-openapi-gen-plan.md` | 特性预研 | 基于 OpenAPI 契约逆向生成测试套件 | Backlog (P2) |
    | `2026-10-28-ai-failure-triage-and-attribution-prd.md` | 核心分诊 | 提出特征指纹聚类算法与 Triage 分诊模型 | `src/triage.rs` |
    | `2026-10-28-ai-failure-triage-and-attribution-dev-plan.md` | 实施细节 | 分诊报告数据结构与字段规范 | `src/attribution.rs` |
    | `2026-10-28-ai-e2e5-master-execution-plan.md` | 交付验收 | 完整的端到端闭环验证与质量看板 | `tests/ai_integration_test.rs` |
    | `ai_distillation_prd.md` | 知识蒸馏 | 测试用例语义提炼与知识沉淀规划 | Backlog (P3) |

### 2.2 主线二：TUI 终端交互演进线 (Terminal User Interface Thread)
*   **现行统一架构档案**：[`doc/architecture/features/tui_architecture.md`](../architecture/features/tui_architecture.md)
*   **演变全貌**：
    原型设计 (`tui_iteration_v2`) → V3 架构重构（M-V-VM 分层与按键解耦）→ 流式调试面板 (`streaming-debug`) → CLI 与 TUI 特性全量对齐 (`tui-cli-parity`)。
*   **涵盖的历史方案追溯矩阵**：
    | 历史方案文件名 | 对应阶段 | 核心贡献与演进结果 | 吸收落地位置 |
    | :--- | :--- | :--- | :--- |
    | `tui_iteration_v2.md` | 原型期 | 早期多窗格概念与基础布局尝试 | 已被 V3 架构取代 |
    | `2026-10-18-tui-design-v3.md` | 核心重构 | 确立 M-V-VM 模式、离屏测试与无状态 Widget 规范 | `src/tui/` |
    | `2026-10-30-tui-streaming-debug-prd.md` | 流式拓展 | SSE 打字机与流式帧渲染缓存 (`StreamState`) | `src/tui/state/stream.rs` |
    | `2026-11-05-tui-cli-parity-prd.md` | 功能对齐 | 补齐 TUI 触发诊断与 Mock 管理的核心能力 | `src/tui/handlers/` |

### 2.3 主线三：批处理执行引擎与工作流演进线 (Runner & Workflow Thread)
*   **现行统一架构档案**：[`doc/architecture/features/batch_and_workflow.md`](../architecture/features/batch_and_workflow.md)
*   **演变全貌**：
    单文件运行 → 目录递归扫描与 DAG 拓扑排序 (`multi-file-testing`) → 动态流控与脚本设计 (`scripting-and-flow-control`) → Markdown 全生命周期提取与双向契约 (`markdown-lifecycle`) → 启发式自动拆分 (`heuristic-auto-split`) → 增量智能测试 (`smart-diff`)。
*   **涵盖的历史方案追溯矩阵**：
    | 历史方案文件名 | 对应阶段 | 核心贡献与演进结果 | 吸收落地位置 |
    | :--- | :--- | :--- | :--- |
    | `2026-06-04-multi-file-testing-design.md` | 批处理 | 递归扫描、依赖图构建与拓扑排序调度 | `src/runner/workflow.rs` |
    | `2026-06-05-sprint3-scripting-and-flow-control-prd.md` | 流控设计 | 引入变量穿透、Fail-Fast 熔断与循环机制 | `src/runner/batch.rs` |
    | `2026-06-06-markdown-lifecycle-specification.md` | 契约驱动 | 定义 Markdown 中提取与更新 HTTP 块规则 | `src/parser/` |
    | `2026-06-11-markdown-lifecycle-development-spec.md` | 规格细化 | 双向导出与无损就地替换算法 | `src/parser/` |
    | `2026-06-16-heuristic-auto-split-prd.md` | 规模化 | 大型单文件用例智能拆解算法 | Backlog (P2) |
    | `2026-11-04-p1-batch-smart-diff-ai-wiring-generator-semantics-prd.md` | 增量调度 | 基于 Git 变更集智能筛选受影响测试 | `src/runner/batch.rs` |

### 2.4 主线四：排障诊断、Mock 与网络重放演进线 (Debug, Mock & Replay Thread)
*   **现行统一架构档案**：[`doc/architecture/features/debug_and_diagnostics.md`](../architecture/features/debug_and_diagnostics.md) 与 [`doc/architecture/features/protocols_and_mock.md`](../architecture/features/protocols_and_mock.md)
*   **演变全貌**：
    Mock 服务器设计 (`mock-server-design`) → 网络连通性细粒度测量 (`network-diagnostics`) → WebSocket 全双工流断言 (`websocket-support`) → 快照重放与本地存储 (`snapshot-replay`) → 单条用例就地重放 (`debug-chain-r6-replay`)。
*   **涵盖的历史方案追溯矩阵**：
    | 历史方案文件名 | 对应阶段 | 核心贡献与演进结果 | 吸收落地位置 |
    | :--- | :--- | :--- | :--- |
    | `2026-06-06-mock-server-design.md` | Mock 内核 | Trie 树路径匹配与条件变体多分支响应 | `src/mock/` |
    | `2026-06-21-url-resolution-and-shortcuts.md` | URL 解析 | 环境前缀拼接与快捷短链展开 | `src/variable/` |
    | `2026-06-23-network-diagnostics-and-stream-capture.md` | 诊断工具 | DNS/TCP/TLS/TTFB 原生测量与 X.509 解析 | `src/http/diagnose.rs` |
    | `2026-06-24-websocket-support-design.md` | 实时协议 | WebSocket 握手、帧监听与操作符软匹配 | `src/ws/` |
    | `2026-07-01-snapshot-replay-roadmap.md` | 历史重现 | 执行上下文快照与单条精准重放机制 | `src/history/` |
    | `2026-07-11-history-storage-optimization-design.md` | 存储优化 | 轻量级文件持久化与索引淘汰算法 | `src/history/` |
    | `2026-10-28-developer-debug-experience-prd.md` | 调试体验 | 错误现场就地归因与 `--explain` 选项 | `src/runner/executor.rs` |
    | `2026-11-04-debug-chain-r6-replay-per-entry-prd.md` | 重放细化 | 基于 Entry ID 的无副作用局部重放 | `src/history/` |

### 2.5 主线五：测试战役、工程规范与质量保障线 (Quality & Testing Thread)
*   **现行统一规范档案**：[`doc/dev/development_workflow.md`](../dev/development_workflow.md)
*   **演变全貌**：
    测试策略规划 (`test-strategy-plan`) → P0 核心链路加固 (`test-p0-execution`) → 质量与防劣化审计 (`quality-hardening`) → 全项目集成与矛盾审计 (`2026-10-26-project-integration-report.md`)。
*   **涵盖的历史方案追溯矩阵**：
    | 历史方案文件名 | 对应阶段 | 核心贡献与演进结果 | 吸收落地位置 |
    | :--- | :--- | :--- | :--- |
    | `2026-10-18-quality-hardening-execution-plan.md` | 防劣化 | 建立冒烟测试脚本 `tests/verify_features.sh` | `tests/` |
    | `2026-10-25-test-strategy-plan.md` | 测试战略 | 划分单元测试、集成测试与 E2E 验证三层金字塔 | CI 流程与 Makefile |
    | `2026-10-25-test-p0-execution.md` | 盲区清零 | 核心用例覆盖率提升战役，锁定边界异常 | `tests/` |
    | `performance_testing.md` | 性能基准 | 建立 Criterion 性能压测基准与回归门禁 | `benches/` |

---

## 3. 规划目录结构与归档分布

```text
doc/plans/
├── README.md                      # [本文档] 演进总纲与交叉引用索引枢纽
├── progress_summary.md            # [长青 SSOT] 项目唯一的进度事实来源
├── future_backlog.md              # [长青待办] 统一需求与技术债务池
├── rupost_product_strategy.md     # [长青战略] 产品战略与竞争定位
├── 2026-11-10-redaction-extensibility-prd.md                  # [当前活跃] 凭据脱敏判定可扩展性与掩码可读性 PRD
├── 2026-11-10-redaction-extensibility-implementation-plan.md  # [当前活跃] 脱敏扩展性技术实施方案与盲区攻坚
├── active/                        # [当前活跃] 正在进行设计评审与实现的方案集
│   ├── 2026-11-09-full-scenario-architecture-review-and-roadmap.md
│   ├── 2026-11-09-implementation-plan-milestones-1-to-3.md
│   ├── 2026-11-09-plugin-architecture-and-extensibility-plan.md
│   ├── 2026-11-06-web-ui-md-editor-prd.md
│   └── 2026-11-06-web-ui-technical-proposal.md
└── archive/                       # [历史归档] 100% 保留全部历史 PRD 原文，按里程碑分层
    ├── 2026-q2/                   # 2026 年 4-6 月份基础引擎早期设计 (11 篇)
    ├── 2026-q3/                   # 2026 年 7-9 月份排障、重放与性能方案 (6 篇)
    ├── 2026-q4/                   # 2026 年 10-11 月 AI、TUI 与质量加固战役方案 (32 篇)
    └── legacy_research/           # 原离散目录 (cookie, folder, script, debug 等) 原始草案 (30+ 篇)
```
