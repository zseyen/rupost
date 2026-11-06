# RuPost 文档知识库总览 (Knowledge Central Hub)

欢迎来到 **RuPost** 项目文档库。
RuPost 是一个基于纯文本文档（`.http` 与 `.md`）驱动的极简高性能 API 测试与回归引擎。
本文档库遵循 **Clean Architecture** 分层理念与 **Diátaxis 架构模型** 组织，消解信息碎片，确保所有设计演进、工程规范与代码落地具备清晰、唯一的单一事实来源 (Single Source of Truth, SSOT)。

---

## 1. 核心导航矩阵 (Navigation Matrix)

无论您是初次上手的使用者、日常迭代的开发者、还是探索架构的设计者，均可依据下表快速直达：

```text
                                  【doc/ 知识中枢】
                                         │
        ┌──────────────────┬─────────────┴────────────┬──────────────────┐
        ▼                  ▼                          ▼                  ▼
  【1. 用户与操作】   【2. 系统核心架构】        【3. 研发流程规范】   【4. 演进规划跟踪】
     guides/          architecture/                dev/                 plans/
   (各场景实战手册)  (7大权威特性档案&契约)    (工作流与工程守则)   (进度SSOT&历史追溯)
```

| 访问目标 | 建议入口文档 | 涵盖内容与价值 |
| :--- | :--- | :--- |
| **💡 快速上手与场景实操** | [`doc/guides/user_guide.md`](guides/user_guide.md)<br>[`doc/guides/batch_testing.md`](guides/batch_testing.md) | 从单请求调试到大规模目录批处理、变量多级覆盖的端到端教程 |
| **🔍 调试、诊断与 Mock** | [`doc/guides/debug_guide.md`](guides/debug_guide.md)<br>[`doc/guides/mock_server.md`](guides/mock_server.md)<br>[`doc/guides/network_diagnostics.md`](guides/network_diagnostics.md) | 深入使用网络连通性瀑布图、单条用例重放、多分支条件 Mock 服务 |
| **🏛️ 深入系统核心架构** | [`doc/architecture/master_plan.md`](architecture/master_plan.md)<br>[`doc/architecture/directory_structure.md`](architecture/directory_structure.md) | 掌握 Clean Architecture 分层依赖、物理目录职责与请求处理生命周期 |
| **📦 核心特性三段式档案** | [`doc/architecture/features/`](architecture/features/) | **7 大权威业务档案**：需求是什么、已完成什么、后续待办什么 |
| **🛠️ 参与贡献与开发规范** | [`doc/dev/development_workflow.md`](dev/development_workflow.md)<br>[`doc/dev/QUICK_REFERENCE.md`](dev/QUICK_REFERENCE.md) | 严格遵循的 Stage 0~6 研发工作流、TDD 流程、JJ 提交与三层测试守则 |
| **📈 查看当前进度与路线** | [`doc/plans/progress_summary.md`](plans/progress_summary.md)<br>[`doc/plans/README.md`](plans/README.md) | 项目唯一进度事实来源 (SSOT)、5 大演进主线、未来待办池 |
| **🌐 行业竞争与差异化定位**| [`doc/competition/`](competition/)<br>[`doc/plans/rupost_product_strategy.md`](plans/rupost_product_strategy.md) | 与 Hurl、HTTPie、ain、ATAC、slumber 的深度全方位对比矩阵 |

---

## 2. 7 大核心领域三段式技术档案 (Domain Feature Dossiers)

针对项目中成熟落地的核心业务领域，我们对历史多阶段草案进行了深度萃取与汇总，提炼出 7 篇标准三段式（需求是什么 / 当前代码现状 / 后续路线图）权威技术档案：

1. 🍪 **[Cookie 与跨请求会话管理全貌](architecture/features/cookie_and_session.md)**
   *涵盖：RFC 6265 注入、fs2 文件咨询锁保护、AutoPersist/Ephemeral 模式、并发克隆隔离。*
2. 🗂️ **[批处理执行引擎与工作流编排全貌](architecture/features/batch_and_workflow.md)**
   *涵盖：递归目录扫描、Kahn 算法 DAG 拓扑排序与环路熔断、顺序/并行双模、Fail-Fast。*
3. 🤖 **[AI 智能赋能体系全貌](architecture/features/ai_engine_system.md)**
   *涵盖：`crates/rupost-ai` 嵌入式架构、自然语言转断言、失败指纹聚类与分诊 (`ai triage`)。*
4. 🩺 **[全链路排障、网络诊断与历史重放全貌](architecture/features/debug_and_diagnostics.md)**
   *涵盖：DNS/TCP/TLS 瀑布图测量 (`rupost diagnose`)、X.509 证书体检、快照持久化与单条就地重放。*
5. 🔀 **[协议扩展体系——Mock、WebSocket 与 SSE 全貌](architecture/features/protocols_and_mock.md)**
   *涵盖：Axum 驱动的 Trie 树动态路由 Mock 服务、多分支变体响应、WebSocket/SSE 流式断言。*
6. 🖥️ **[终端交互系统 (TUI) 架构全貌](architecture/features/tui_architecture.md)**
   *涵盖：M-V-VM 架构分层、Ratatui 无状态 Widget 渲染、流式 Word-Wrap 预折行、离屏无头测试。*
7. 🔌 **[插件体系与开放扩展全貌](architecture/features/plugin_and_extensibility.md)**
   *涵盖：Micro-Kernel 扩展点、Manifest 清单规约、零信任分级权限模型与 Wasm 沙箱规划。*

---

## 3. 文档目录职责与治理规则 (Directory Governance)

```text
doc/
├── README.md                      # [本文档] 全局知识导航地图
├── guides/                        # 用户指南（保持路径稳定，对齐 README 链接）
├── architecture/                  # 系统核心架构设计
│   ├── master_plan.md             # 顶层架构设计蓝图
│   ├── directory_structure.md     # 物理与逻辑目录映射指南
│   ├── http_md_role_contract.md   # HTTP 与 Markdown 职责契约
│   └── features/                  # 7 大权威领域三段式技术档案
├── dev/                           # 工程与研发流程
│   ├── development_workflow.md    # 核心研发流程规范 (Stage 0~6)
│   ├── QUICK_REFERENCE.md         # 开发者速查手册
│   └── github_actions.md          # CI 自动化流水线
├── competition/                   # 竞品分析与行业战略
├── templates/                     # 模版库 (prd-template.md 由 Makefile 动态引用)
└── plans/                         # 演进规划中枢 (100% 保留全部历史方案)
    ├── README.md                  # 演进主线总纲与交叉引用索引
    ├── progress_summary.md        # 唯一开发进度事实来源 (SSOT)
    ├── future_backlog.md          # 统一未来待办需求池
    ├── active/                    # 正在讨论与推进中的活跃 PRD
    └── archive/                   # 历史交付方案分期归档 (2026-q2/q3/q4 & legacy_research/)
```

### 文档维护契约
1. **单一事实来源原则**：所有开发进度的新增与闭环，必须且仅能记录于 [`doc/plans/progress_summary.md`](plans/progress_summary.md)；未排期想法统一录入 [`doc/plans/future_backlog.md`](plans/future_backlog.md)。
2. **规范模板契约**：所有新功能必须先通过 `make prd` 生成标准模版并进行架构评审，确认后再进入代码实现。
3. **零破损原则**：保持各子目录中的重定向说明与历史原案链接通畅，确保外部工具链（Makefile、CI 脚本）稳定运行。
