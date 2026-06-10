# RuPost 进度汇总 (Progress Summary)

本文件作为 RuPost 项目唯一的开发进度事实来源，记录已完成的阶段任务、未完成的计划以及未来的架构路线。

---

## 进度概览

| 阶段 / 功能模块 | 目标与核心特征 | 状态 | 交付物 |
| :--- | :--- | :--- | :--- |
| **Sprint 1: 核心重构与优化** | 引入 User-Agent 动态配置与 Request 级覆盖，收敛 Cookie 管理管道 | 已完成 | `src/http/client.rs`, `src/runner/executor.rs`, `tests/user_agent_test.rs` |
| **Sprint 2: 多文件与目录递归测试** | 递归扫描过滤、有向无环图 (DAG) 拓扑排序、顺序/并行双执行模式、并发 Cookie 隔离保护、Fail-Fast 流程中断、JSON 结构化报告、递归依赖补全与安全沙箱、并行状态克隆传递 | 已完成 | `src/runner/scanner.rs`, `src/runner/workflow.rs`, `src/runner/batch.rs`, `src/runner/resolver.rs`, `src/runner/parallel.rs`, `tests/batch_testing_test.rs` |
| **Sprint 3: 高级特性与脚本引擎** | @loop 循环, @skip-if 条件运行, 前后置 Javascript/Rust 脚本支持 | 未开始 | - |
| **Sprint 4: HTML 报告与高级表现层** | 导出可视化 HTML 报告与模板表现层 | 未开始 | - |

---

## 已交付细节

### Sprint 1: 核心重构与优化
1.  **动态 User-Agent 配置**：默认发送 `rupost/1.0.0` 作为 UA。支持从 `VariableContext` 读取全局 `user_agent` 变量并覆盖，以及在用例中显式使用 `User-Agent` 头部进行最高优先级覆盖。
2.  **Cookie 状态管理收敛**：剥离并封装 `CookieMiddleware` 交互，TestExecutor 不再硬编码 cookie store 的细节，完全符合 Clean Architecture。

### Sprint 2: 多测试文件与目录递归测试
1.  **DirectoryScanner**：支持递归遍历多个目录，自动过滤 `.http`/`.md`，主动剔除 `.git/`, `.rupost/`, `target/` 等隐藏或无关路径，保证按字典序升序。
2.  **WorkflowGraph (DAG & Kahn)**：解析用例文件头部的 `### @depends-on <filename>` 属性，使用 Kahn 拓扑排序算法计算执行顺序，并具备高强度的循环依赖环路自检。
3.  **DependencyResolver & 安全沙箱 (Sandbox Scope Jail)**：自动递归加载并补全缺失的依赖测试文件；强制校验依赖绝对路径必须位于沙箱目录范围内，并进行后缀合法性审查，切断路径穿越（Path Traversal）安全漏洞。
4.  **BatchExecutor**：
    *   **顺序模式**：按拓扑序串行运行，变量及 Cookie 自然传递。
    *   **并行模式**：利用 `tokio::task::JoinSet` 及并发限制器进行并发处理。
    *   **状态克隆传递 (State Cloning)**：在拓扑依赖链上，实现从前置节点向子节点单向克隆并合并 Context 变量增量与序列化还原的会话 Cookie，彻底消除并行开发的数据孤岛，而非依赖分支依然维持安全隔离。
    *   **Fail-Fast 控制**：支持随时在串行或并行模式下检测到错误请求时中断其余用例。
5.  **结构化 JSON 报告与批汇总**：支持 `--report json` 格式的控制台或文件输出；支持终端高颜值批测试摘要输出。
