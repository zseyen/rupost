# 全场景自适应引擎与微内核插件生态任务总纲 (Task Nexus)

- **任务代号**: `cross-scenario-engine-and-plugins`
- **创建日期**: 2026-11-10
- **管理目录**: `doc/plans/2026-11-10-cross-scenario-engine-and-plugins/`
- **任务状态**: Active / Draft (v2.1.0 修订稿待评审——M1 开工前须完成人工评审，通过后回标 Execution Ready)

---

## 0. 文档治理与权威矩阵 (Governance, v2.1.0)

| 权威域 | 唯一事实来源 |
| :--- | :--- |
| **MVP 范围与施工顺序** | [06 §3/§4](./06-mvp-implementation-plan-personas-and-scenarios.md) |
| **快照产物 Schema** | [02 §2](./02-artifact-contract-for-plugins.md) |
| **插件发现/IPC 协议/降级语义/执行仲裁** | [03](./03-plugin-spi-and-degradation.md) |
| **基础能力与 API 现状基线** | [01](./01-core-capabilities-and-api.md) |
| **测试防护网** | [04](./04-testing-strategy-and-scenarios.md) |
| **ADR 决策与风险登记** | [05](./05-comprehensive-architecture-and-requirements-summary.md)（已从白皮书降级为 ADR 索引） |

* **基线标注约定**：所有能力条目必须标注 `[已有]`（附 file:line 锚点）或 `[待建 M<N>]`——禁止把待建能力写成既有能力（v2.0.0 的核心失真模式）；
* 对抗性审查记录（4 路子代理独立攻击 + 交叉验证 + 修订索引）：[review-adversarial.md](./review-adversarial.md)；
* **P0/P1 缺陷追踪底账**（解决方案 + 落点 + 验收，M5 结束全表 ✅ 方可回标 Execution Ready）：[p0-p1-remediation-plan.md](./p0-p1-remediation-plan.md)。

---

## 1. 任务背景与核心使命

针对用户在真实世界中的三大异构环境（开发环境、发布环境、调试环境），彻底确立 `rupost` 的**“微内核自包含基础功能体系”**、**“离线产物数据黑匣子契约”**、**“统一注册表生态 (DeepSeek Harness 模式)”** 与 **“AI 预留接口体系”**，确保：
1. **开发环境 (Full Dev)**：插件最全、功能最完整（语义化动态鉴权/加签、CLI 扩展命令、TUI 探索、AI 原位归因）；
2. **发布环境 (Pipeline CI)**：单二进制基础功能，零外部依赖闭环执行回归与门禁判定，输出工业级 JUnit XML；
3. **调试环境 (Portable Debug)**：即拷即走，环境严重不一致且无外部运行时（无 Python/Node/网络）时依然 100% 独立可用；
4. **【核心枢纽】产物挖掘契约**：基础功能生成的测试产物（Snapshot / Trace）完整保真现场数据，能无缝供开发环境的插件进行深层次挖掘与二次分析，绝不因调试环境缺失插件导致现场信息丢失或测试阻塞。

---

## 2. 决策与规划文档矩阵 (Decision Records Index)

本任务文件夹下包含以下 6 份核心决策与执行文档：

| 序号 | 文档名称 | 核心职责与治理范围 |
| :---: | :--- | :--- |
| **01** | [`01-core-capabilities-and-api.md`](./01-core-capabilities-and-api.md) | **微内核自包含基础功能清单与 API 边界**：定义 0 外部依赖下的绝对核心能力、CLI 自定义子命令与 Crate API 接口。 |
| **02** | [`02-artifact-contract-for-plugins.md`](./02-artifact-contract-for-plugins.md) | **离线产物黑匣子契约与插件挖掘规范**：定义 Snapshot/Trace 产物 Schema 及插件二次消费挖掘机制。 |
| **03** | [`03-plugin-spi-and-degradation.md`](./03-plugin-spi-and-degradation.md) | **插件 SPI 体系、统一注册表与生命周期管线规范**：定义 DeepSeek Harness 注册表、不可逆阶段顺序流、CLI 自定义命令与 AI 预留 SPI。 |
| **04** | [`04-testing-strategy-and-scenarios.md`](./04-testing-strategy-and-scenarios.md) | **全场景端到端测试方案与防护网**：针对裸单二进制、断网即拷即走、产物跨机挖掘的完整测试用例。 |
| **05** | [`05-comprehensive-architecture-and-requirements-summary.md`](./05-comprehensive-architecture-and-requirements-summary.md) | **全场景自适应架构与插件微内核技术决策白皮书**：汇总产品需求、技术选型权衡、Clean Architecture 评估与核心风险难点攻坚。 |
| **06** | [`06-mvp-implementation-plan-personas-and-scenarios.md`](./06-mvp-implementation-plan-personas-and-scenarios.md) | **插件体系 MVP 开发计划、用户画像与场景规约**：刻画 4 类真实用户画像、4 大端到端场景流转及 5 阶段原子化实施路线图。 |

---

## 3. 三大环境能力映射与降级矩阵

| 功能维度 | 场景 1：开发环境 (Full Dev) | 场景 2：发布环境 (Pipeline CI) | 场景 3：调试环境 (Portable Debug) |
| :--- | :--- | :--- | :--- |
| **运行时依赖** | 系统已安装 Python、Node、AWS CLI 等 | 容器环境，仅保证基础 glibc/musl | **纯静态单二进制，0 外部依赖，即拷即走** |
| **请求加签与变形** | `@auth` / `@sign` / `@transform` 插件调用 | 纯 Rust 内置宏（`$hmac_sha256`、`$sha256`）`[待建 M1]` | 内置宏优先；外置插件缺失时按 03 §2.1 声明降级（原样 Header 外发 + 三处 WARNING） |
| **执行顺序控制** | `Prepare` $\to$ `Transform` $\to$ `Sign` 阶段保序 | 纯内置宏确定性执行 | 阶段顺序保证 + 缺失优雅降级 |
| **CLI 命令扩展** | 注册表双模路由 (`rupost flamegraph`) | 仅调用内置核心子命令 | 离线仅录制产物，不调用外部扩展命令 |
| **AI 接口预留** | 创作期 `@ai-assert` / 离线黑匣子归因分诊 | 严禁运行期 AI (B0 铁律) | 严禁运行期 AI (B0 铁律) |
| **网络时序** | 微米级 DNS/TCP/TLS/TTFB 瀑布图 | 汇总平均 TTFB 与慢请求分诊 | 完整录制物理寻址、Socket 握手与首包耗时 |
| **产物输出** | 终端高审美渲染 + 实时 HTML 活文档 | 标准 `junit.xml` + 机器可读 `report.json` | **`--save-snapshot` 导出包含全量现场的 JSON 黑匣子** |
| **深度信息挖掘** | 本地插件直接读取快照进行 AI 归因、可视化 | 流水线归档测试报告 | **不执行本地挖掘，无损携带产物回传开发机** |
