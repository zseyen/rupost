# 全场景自适应引擎与微内核插件生态任务总纲 (Task Nexus)

- **任务代号**: `cross-scenario-engine-and-plugins`
- **创建日期**: 2026-11-10
- **管理目录**: `doc/plans/2026-11-10-cross-scenario-engine-and-plugins/`
- **任务状态**: Active / Execution Ready

---

## 1. 任务背景与核心使命

针对用户在真实世界中的三大异构环境（开发环境、发布环境、调试环境），彻底确立 `rupost` 的**“微内核自包含基础功能体系”**、**“离线产物数据黑匣子契约”**与**“插件扩展生态”**，确保：
1. **开发环境 (Full Dev)**：插件最全、功能最完整（动态加签、Mock 原生 CORS、AI 原位归因、TUI 探索）；
2. **发布环境 (Pipeline CI)**：单二进制基础功能，零外部依赖闭环执行回归与门禁判定，输出工业级 JUnit XML；
3. **调试环境 (Portable Debug)**：即拷即走，环境严重不一致且无外部运行时（无 Python/Node/网络）时依然 100% 独立可用；
4. **【核心枢纽】产物挖掘契约**：基础功能生成的测试产物（Snapshot / Trace）完整保真现场数据，能无缝供开发环境的插件进行深层次挖掘与二次分析，绝不因调试环境缺失插件导致现场信息丢失或测试阻塞。

---

## 2. 决策与规划文档矩阵 (Decision Records Index)

本任务文件夹下包含以下 4 份核心决策与执行文档：

| 序号 | 文档名称 | 核心职责与治理范围 |
| :---: | :--- | :--- |
| **01** | [`01-core-capabilities-and-api.md`](./01-core-capabilities-and-api.md) | **微内核自包含基础功能清单与 API 边界**：定义 0 外部依赖下的绝对核心能力与 API 接口。 |
| **02** | [`02-artifact-contract-for-plugins.md`](./02-artifact-contract-for-plugins.md) | **离线产物黑匣子契约与插件挖掘规范**：定义 Snapshot/Trace 产物 Schema 及插件二次消费挖掘机制。 |
| **03** | [`03-plugin-spi-and-degradation.md`](./03-plugin-spi-and-degradation.md) | **插件 SPI 体系与环境自愈降级策略**：定义 Stdio IPC 管道协议及缺失插件时的优雅降级规则。 |
| **04** | [`04-testing-strategy-and-scenarios.md`](./04-testing-strategy-and-scenarios.md) | **全场景端到端测试方案与防护网**：针对裸单二进制、断网即拷即走、产物跨机挖掘的完整测试用例。 |

---

## 3. 三大环境能力映射与降级矩阵

| 功能维度 | 场景 1：开发环境 (Full Dev) | 场景 2：发布环境 (Pipeline CI) | 场景 3：调试环境 (Portable Debug) |
| :--- | :--- | :--- | :--- |
| **运行时依赖** | 系统已安装 Python、Node、AWS CLI 等 | 容器环境，仅保证基础 glibc/musl | **纯静态单二进制，0 外部依赖，即拷即走** |
| **请求加签** | `@plugin aws-sigv4`（调用本地 SDK 算签） | 纯 Rust 内置宏（`$hmac_sha256`、`$sha256`） | 内置宏优先；外置插件支持声明降级或使用预留 Token |
| **执行调度** | 并发拓扑执行 + 单请求 `--only` 原位调试 | 确定性 DAG 拓扑 + `--fail-fast` 快速熔断 | 确定性 DAG 执行 + `@teardown` 现场终态数据清理 |
| **网络时序** | 微米级 DNS/TCP/TLS/TTFB 瀑布图 | 汇总平均 TTFB 与慢请求分诊 | 完整录制物理寻址、Socket 握手与首包耗时 |
| **产物输出** | 终端高审美渲染 + 实时 HTML 活文档 | 标准 `junit.xml` + 机器可读 `report.json` | **`--save-snapshot` 导出包含全量现场的 JSON 黑匣子** |
| **深度信息挖掘** | 本地插件直接读取快照进行 AI 归因、可视化 | 流水线归档测试报告 | **不执行本地挖掘，无损携带产物回传开发机** |
