# Rupost 全场景自适应架构与插件微内核技术决策白皮书

- **文档代号**: `cross-scenario-engine-and-plugins-summary`
- **归档目录**: `doc/plans/2026-11-10-cross-scenario-engine-and-plugins/`
- **基线版本**: v2.0.0 (Execution Ready - Integrated Registry, Pipeline, Custom Commands & AI SPI)
- **关联文档**:
  - [`README.md`](./README.md)
  - [`01-core-capabilities-and-api.md`](./01-core-capabilities-and-api.md)
  - [`02-artifact-contract-for-plugins.md`](./02-artifact-contract-for-plugins.md)
  - [`03-plugin-spi-and-degradation.md`](./03-plugin-spi-and-degradation.md)
  - [`04-testing-strategy-and-scenarios.md`](./04-testing-strategy-and-scenarios.md)
  - [`06-mvp-implementation-plan-personas-and-scenarios.md`](./06-mvp-implementation-plan-personas-and-scenarios.md)
  - [`ai-master-roadmap.md`](../archive/2026-q4/2026-10-27-ai-master-roadmap.md)

---

## 目录 (Table of Contents)

1. [产品需求 (Product Requirements)](#1-产品需求-product-requirements)
   - 1.1 项目愿景与第一性原理
   - 1.2 三大异构场景深度拆解
   - 1.3 核心需求与能力矩阵
   - 1.4 产物契约与离线深层挖掘价值
2. [技术选型与权衡 (Technical Selection & Tradeoffs)](#2-技术选型与权衡-technical-selection--tradeoffs)
   - 2.1 插件架构 4 大方案横向对比
   - 2.2 为什么方案 D (Unix Stdio IPC) 胜出
   - 2.3 借鉴工业级先例：DeepSeek Harness、Git 与 GitHub CLI
   - 2.4 内核基础辅助依赖选型决策
3. [架构评估与 Clean Architecture 实现 (Architectural Assessment)](#3-架构评估与-clean-architecture-实现-architectural-assessment)
   - 3.1 Clean Architecture 分层映射
   - 3.2 微内核 7 大自包含基础功能底座
   - 3.3 DeepSeek Harness 统一注册表 (PluginRegistry) 架构
   - 3.4 插件执行顺序控制体系 (Stage + Priority + Patch) 与友好语法 (IntelliSense)
   - 3.5 CLI 自定义命令双模动态路由与 TTY 透传 (Custom Commands SPI)
   - 3.6 AI 预留接口体系 (Context, Miner, Arbiter, MCP Bridge)
   - 3.7 优雅降级与环境自愈协议
4. [风险、模糊点与关键技术难点 (Risks, Ambiguities & Technical Challenges)](#4-风险模糊点与关键技术难点-risks-ambiguities--technical-challenges)
   - 4.1 阻断性风险 1：JSON 请求体重排篡改破坏签名 (Phase 0 决策 A)
   - 4.2 阻断性风险 2：现场快照静默吞没网络失败/超时请求
   - 4.3 安全模糊点：项目级本地插件恶意执行 (RCE) 防范
   - 4.4 关键技术难点：大包管道并发死锁与插件冷启动时延隔离
   - 4.5 跨平台路径与 CRLF 换行污染防护
5. [总结与路线图规划 (Roadmap & Milestones)](#5-总结与路线图规划-roadmap--milestones)

---

## 1. 产品需求 (Product Requirements)

### 1.1 项目愿景与第一性原理

`rupost` 的核心使命是成为**“基于纯文本 HTTP 文档驱动的下一代轻量级 API 测试与自动化工具”**。
从第一性原理出发，API 测试的本质是：
1. **输入阶段**：解析直观、符合 RFC 7230 的纯文本 `.http` 文档，进行变量插值与动态计算；
2. **执行阶段**：以确定性 DAG 依赖拓扑高并发驱动网络 I/O，并以微米级精度感知物理链路（DNS、TCP、TLS、TTFB）；
3. **判定阶段**：基于丰富的断言引擎验证响应状态、Headers、JSONPath 乃至 Schema 契约；
4. **输出阶段**：生成既具备终端极致审美、又具备机器确定性消费（JUnit XML、Snapshot 离线黑匣子）的数据产物。

我们坚持极简设计，拒绝将非必要重量级依赖（如大型脚本引擎、复杂沙箱、重型外部数据库）塞入内核。

---

### 1.2 三大异构场景深度拆解

用户在实际软件生命周期中面对着完全不同的三类异构环境，`rupost` 必须提供自适应支撑：

```
+----------------------------------------------------------------------------------------------------+
|                                    三大异构场景自适应架构                                          |
+----------------------------------------------------------------------------------------------------+
| 场景 1: 开发环境 (Full Dev)      | 场景 2: 发布环境 (Pipeline CI)      | 场景 3: 调试环境 (Portable Debug) |
| - 插件最全、生态最丰富           | - 0 外部依赖单二进制                | - 单二进制“即拷即走”              |
| - 动态 AWS SigV4 / 外部 Python   | - 严格断言门禁，确定性退出码        | - 断网 / 异构无 Python、Node 环境 |
| - Mock 原生智能 CORS             | - 工业级 JUnit XML 报告输出         | - 无损录制全量现场数据黑匣子      |
| - TUI 交互式探索与原位调试       | - 确定性流水线执行与并发熔断        | - 离线携带产物回传开发机深度挖掘  |
+----------------------------------------------------------------------------------------------------+
```

#### 场景 1：开发环境 (Full Dev)
- **目标用户**：API 开发者、自动化测试工程师。
- **环境特征**：本地装有 Python、Node.js、AWS CLI、Git 等完备的开发者工具链。
- **核心诉求**：
  - 功能最全，体验最丝滑，支持丰富的扩展插件（如复杂的 AWS SigV4 认证、团队专属私有加签算法、AI 原位归因诊断、文档一键同步）。
  - 支持交互式 TUI 探索、单请求原位重发（`--only`）、智能 Mock 服务自动补全 CORS 头。

#### 场景 2：发布环境 (Pipeline CI/CD)
- **目标用户**：持续集成系统（GitHub Actions, GitLab CI, Jenkins, Kubernetes Runner）。
- **环境特征**：轻量级容器（Alpine, Debian-Slim, Scratch），仅具备基础系统调用，网络隔离严格，**绝无预装的外部脚本运行时**。
- **核心诉求**：
  - **单静态二进制自包含**，无任何动态链接库陷阱（纯静态 musl/glibc 兼容）。
  - **0 外部依赖执行闭环**：无需调用外部命令即可执行 100% 的基础测试流程（内置 `$sha256`、`$hmac_sha256`、`$uuid` 等基础函数）。
  - **确定性门禁判定**：标准的 Linux Exit Code（0=全部通过，1=断言失败，2=配置语法错误）。
  - **标准机器可读报告**：输出合规的 JUnit XML（供 Jenkins/GitLab 渲染测试大盘）与 JSON 摘要。

#### 场景 3：调试环境 (Portable Debug)
- **目标用户**：SRE 运维工程师、一线技术支持、现场排障工程师。
- **环境特征**：客户生产受限网络、堡垒机、内网专线服务器、离线风控机房。**环境与研发机严重不一致，严禁安装 Python/Node 等三方软件，外部网络断绝**。
- **核心诉求**：
  - **即拷即走 (Copy & Run)**：单个可执行文件直接通过 scp/U 盘拷贝即用，零配置启动。
  - **绝不卡死**：遇到使用了开发机专属插件的脚本时，支持 `--allow-missing-plugins` 优雅降级，打印 WARN 并使用预留 Token 推进，绝不直接崩溃。
  - **【核心枢纽】数据黑匣子 (Artifact Blackbox)**：网络失败、连接超时、DNS 无法解析等故障现场，**必须 100% 完整录制物理请求、响应（或错误详情）与底层网络微米级时序**，存为 `snapshot.json`。
  - **离线回传挖掘**：现场无法运行大型插件挖掘深层原因没关系，运维只需将 `snapshot.json` 拷回开发机，开发环境中的富插件（如 AI 智能根因分析器、链路拓扑回放器）可完全基于离线产物进行二次解析，**绝不因为现场环境简陋而丢失事故真相**。

---

### 1.3 核心需求与能力矩阵

| 能力分类 | 场景 1：开发环境 | 场景 2：发布环境 | 场景 3：调试环境 | 需求验收标准 |
| :--- | :--- | :--- | :--- | :--- |
| **运行时依赖** | 宿主语言丰富 (Python/Node/Go) | 裸单二进制 (0 外部依赖) | 裸单二进制 (即拷即走) | 静态编译大小 $\le 20\text{MB}$，内存常驻 $\le 30\text{MB}$ |
| **HTTP 规范解析** | RFC 7230 语法，支持变量插值 | 严格解析，JSON 零拷贝保真 | 严格解析，语法容错并捕获 | 保留原始 JSON 字段顺序与空格格式 (Phase 0) |
| **请求加签能力** | `@auth` / `@sign` 语义化插件调用 | 纯 Rust 内置通用哈希与 HMAC 宏 | 内置宏优先；外置插件支持降级 | 内置 `$sha256`、`$hmac_sha256` 覆盖 80% 签名 |
| **执行顺序控制** | `Prepare` $\to$ `Transform` $\to$ `Sign` 阶段流 | 纯宏确定性执行 | 顺序保证 + 缺失降级 | 阶段内按声明顺序，支持 `priority` 微调 |
| **自定义命令扩展** | 注册表动态分发 (`rupost flamegraph`) | 不调用扩展命令 | 离线仅录制产物，回传挖掘 | 双模动态路由 + TTY 直通透传 |
| **AI 接口预留** | 创作期 `@ai-assert` / 离线黑匣子分诊 | 严禁运行期 AI (B0 铁律) | 严禁运行期 AI (B0 铁律) | 四大 SPI 预留，脱敏清洗强制阻断凭据泄露 |
| **链路时序感知** | 微米级 DNS/TCP/TLS/TTFB 瀑布图 | 汇总平均 TTFB 与慢请求报警 | 逐请求完整录制微米级物理时延 | 记录 DNS、TCP 握手、TLS 握手、首包 TTFB |
| **输出与报告** | 终端 HSL 高级着色 + TUI | 工业级 JUnit XML + `report.json` | JSON 离线数据黑匣子快照 | JUnit XML 兼容 Jenkins/GitLab，快照包含失败现场 |

---

### 1.4 产物契约与离线深层挖掘价值

离线产物契约是联结“极简现场”与“富开发态”的核心纽带。它的价值在于：
1. **测试与分析解耦**：执行引擎只负责高效、确定性地收集现场事实；挖掘插件负责从事实中提取见解；
2. **零现场侵入**：不需要在受限生产机器上安装 500MB 的 Python 依赖或配置复杂的分析环境；
3. **确定性重放**：开发人员可在本地基于 `rupost trace analyze snapshot.json` 重现当时的请求顺序、Headers 与物理耗时，实现真正的“异地远程 Debug”。

---

## 2. 技术选型与权衡 (Technical Selection & Tradeoffs)

### 2.1 插件架构 4 大方案横向对比

针对用户提出的“评估项目基础条件，借鉴业界类似项目实现方案，分析优劣势”，我们对业内常见的四种扩展机制进行了全维度技术选型评估：

| 评估维度 | 方案 A: WebAssembly 沙箱 (Extism/Wasmtime) | 方案 B: 动态链接库 (libloading / C ABI) | 方案 C: 嵌入式脚本语言 (Rhai / mlua / Boa) | 方案 D: Unix Stdio IPC 进程管道 (DeepSeek Harness / Git 模式) |
| :--- | :--- | :--- | :--- | :--- |
| **二进制膨胀** | 极严重：引入 Wasmtime 引擎，单二进制膨胀 20MB~35MB | **极小**：仅需微量 FFI 代码，几乎 0 额外体积 | 较重：引入嵌入式解释器，二进制增加 5MB~12MB | **零体积增加**：仅使用 `tokio::process` 与 `serde_json`，0 外部 Crate |
| **跨平台静态编译** | 极差：涉及 C 绑定与底层 JIT，跨 target 交叉编译极易失败 | 灾难：动态库 ABI 极度脆弱，Windows/Linux/macOS 格式完全不同 | 良好：纯 Rust 引擎编译良好，但受限于解释器能力 | **完美**：跨平台兼容性 100%，纯静态编译零外部动态链接 |
| **插件生态与语言自由度**| 较差：插件作者必须编译为 wasm32-wasi，调试与门槛高 | 较差：仅支持 C/C++/Rust，需要处理指针与内存分配安全 | 差：受限于 DSL/特定语法，无法直接使用已有 Python/Node 官方 SDK | **最优**：插件可以用 **Python/Node/Go/Bash/Rust** 任意语言编写 |
| **调试与现场排障难度** | 黑盒：Wasm 内部崩溃排查困难，无法直接注入日志 | 危险：动态库内存越界直接导致 `rupost` 主进程 Segmentation Fault | 中等：报错信息依赖内嵌解释器调用栈 | **极度透明**：插件输出直接由 `stderr` 打印到终端，可单行调试 |
| **三环境匹配度** | 差：调试环境即拷即走困难 | 差：发布与调试环境动态库找不到 (dlopen error) | 中等：无法在现场调用现有的系统工具 | **极致契合**：缺插件优雅降级，有插件即调即走 |

---

### 2.2 为什么方案 D (Unix Stdio IPC) 胜出

从 `rupost` 的产品定位与第一性原理出发，方案 D 展现出压倒性的工程美感与实用性：
1. **坚守 Clean Architecture 与轻量化原则**：
   无需在 Rust 内核引入重量级外部引擎。主程序始终维持纯粹、极速、无污染的状态，二进制保持在极致小巧的 15MB 左右。
2. **最大化复用现实世界的软件资产**：
   用户需要接入 AWS 复杂的 SigV4、阿里云 OpenAPI 加签、自研 OAuth2 流程。使用方案 D，用户直接在插件中写 15 行 Python 脚本 `import boto3` 或使用 Node.js 库即可完成，无需重写 Rust 绑定位。
3. **进程级绝对安全隔离**：
   插件是一个子进程，它的崩溃、内存泄漏、死循环绝不会带崩 `rupost` 核心进程；通过标准超时机制（`kill_on_drop`）和资源配额，内核具备绝对的进程主控权。
4. **最符合“即拷即走”降级哲学**：
   当 `rupost` 被单独拷到堡垒机上，即便没有 Python 插件，内核也可无损识别该缺失并启动降级运行，绝不会报 `Wasm runtime missing` 或 `libloading error`。

---

### 2.3 借鉴工业级先例：DeepSeek Harness、Git 与 GitHub CLI

方案 D 与注册表架构吸收了计算机软件历史上最健壮的扩展设计精髓：
- **DeepSeek Harness (`dsh`) 注册表模式**：
  核心框架将所有命令、评估器、钩子作为强类型注册项收拢于 `Registry`。元数据轻量自省，运行时按需懒加载；输入输出通过确定性 JSON 规范解耦。
- **Git Filter Process (`clean` / `smudge`)**：
  Git LFS（大文件存储）完全通过标准输入输出与 Git 核心交互，历经数十年跨平台数亿次运行，被验证为史上最可靠的进程交互模型。
- **GitHub CLI (`gh extension`)**：
  `gh` 插件体系完全基于可执行进程协议，在 `gh --help` 优雅呈现扩展清单，并在执行时实现 TTY 直连透传。

---

### 2.4 内核基础辅助依赖选型决策

为了支撑发布环境（CI）与调试环境的自包含能力，我们对关键基础功能依赖进行了极致审慎的选型：
1. **XML 报告生成库**：选择 `quick-xml`（`serialize` 特性），零分配事件流式写入，彻底放弃膨胀老旧的 `xml-rs`。
2. **终端控制字符剥离**：引入极轻量的 `strip-ansi-escapes`，确保写入 XML 和快照的报错信息 100% 纯净，避免破坏机器解析器。
3. **基础哈希加签库**：内核直接引入纯 Rust、零系统依赖的 `sha2`、`hmac`、`md-5`。使 80% 常见的请求签名（如 HMAC-SHA256 签名）无需依赖外部插件即可在 CI 和现场全闭环计算。

---

## 3. 架构评估与 Clean Architecture 实现 (Architectural Assessment)

### 3.1 Clean Architecture 分层映射

我们严格遵循 Clean Architecture 原则，将微内核与插件扩展机制划分为清晰的同心圆结构：

```
+----------------------------------------------------------------------------------------------------+
|                                    Clean Architecture 架构映射                                     |
+----------------------------------------------------------------------------------------------------+
| 4. 框架与外部驱动 (Frameworks & Drivers)                                                            |
|    - Tokio 异步运行时, Reqwest 网络客户端, OS Process 管道 (tokio::process), 物理文件系统          |
+----------------------------------------------------------------------------------------------------+
| 3. 接口适配器层 (Interface Adapters)                                                               |
|    - CLI 解析 (Clap v4), PluginCommandRouter, StdioDriver (管道驱动器), JUnitFormatter              |
+----------------------------------------------------------------------------------------------------+
| 2. 领域用例层 (Application Usecases)                                                               |
|    - TestRunner, PluginRegistry (统一注册表), PluginPipeline (执行流水线),                          |
|      TeardownManager, TimingWaterflowRecorder, AiArbiter (AI建议裁决器)                             |
+----------------------------------------------------------------------------------------------------+
| 1. 核心实体层 (Domain Entities)                                                                     |
|    - Request / Response, PluginPatch, PluginManifest, ExecutionSnapshotFile,                       |
|      TimingWaterfall, DegradationPolicy                                                            |
+----------------------------------------------------------------------------------------------------+
```

- **依赖单向流动规则**：外部驱动只能依赖内层接口，内层实体绝对不感知外部具体协议。
- **Rich Domain Model (充血模型)**：`PluginPatch`、`PluginManifest` 与 `Request` 自带状态判定与补丁合并方法，杜绝外部闭包解构内部私有字段。

---

### 3.2 微内核 7 大自包含基础功能底座

即便在物理断网且缺失任何外部插件的极端受限环境中，`rupost` 内核原生提供 7 大闭环基础能力：
1. **RFC 7230 原生解析引擎**：纯文本 `.http` 文件解析，支持多请求分隔与三反引号多行 Body；
2. **内置动态宏与哈希计算**：`$uuid`, `$timestamp`, `$randomInt`, `$sha256`, `$hmac_sha256`；
3. **DAG 拓扑与并发熔断引擎**：拓扑排序调度，`--fail-fast` 极速熔断，单请求 `--only` 原位执行；
4. **`@teardown` 逆拓扑清理保障**：逆依赖序执行清理请求，即使前序发生断言失败或超时也 100% 触发；
5. **微米级物理时延采集网关**：基于 reqwest/socket 埋点精确记录 DNS, TCP, TLS, TTFB 链路瀑布；
6. **确定性断言与测试闭环**：状态码, Headers, JSONPath, 正则匹配, 机器可读退出码 (0/1/2)；
7. **工业级报告与数据黑匣子**：标准 JUnit XML 报告生成, 全量故障数据保真快照 (Snapshot v1.0)。

---

### 3.3 DeepSeek Harness 统一注册表 (PluginRegistry) 架构

系统引入 `PluginRegistry` 作为全生命周期插件单一事实来源，统一统筹 **CLI 自定义命令**、**请求级注解钩子** 与 **离线产物挖掘器**：

```rust
pub struct PluginRegistry {
    /// 命令路由表: 子命令名 -> 插件元数据
    pub commands: HashMap<String, Arc<CommandCapability>>,
    /// 注解路由表: 注解名 (如 "auth", "sign") -> 插件钩子定义
    pub hooks: HashMap<String, Arc<HookCapability>>,
    /// 挖掘器路由表: 挖掘器标识 -> 挖掘能力定义
    pub miners: HashMap<String, Arc<MinerCapability>>,
}
```

* **极速发现与本地缓存**：扫描 `$PATH` 与 `~/.rupost/plugins`，将 `rupost-plugin-* --manifest` 结果缓存于 `~/.rupost/registry.json`，缓存命中时启动开销 $\le 5\text{ms}$；
* **沙箱警戒与防 RCE**：工作区 `./.rupost/plugins` 默认隔离，必须携带 `--allow-project-plugins` 放行。

---

### 3.4 插件执行顺序控制体系 (Stage + Priority + Patch) 与友好语法 (IntelliSense)

#### 1. 语义化领域语法糖
废除生硬的底层配置注解，转用符合 HTTP 意图的一等公民语法：
* `@auth <name> [params] ?optional`：动态鉴权、Token 获取（归属 `Prepare` 阶段）；
* `@transform <name> [params]`：请求体加密/变形/压缩（归属 `Transform` 阶段）；
* `@sign <name> [params]`：报文防篡改签名、哈希摘要（归属 `Sign` 阶段）；
* `@pipeline a -> b -> c`：跨阶段显式管道链。

#### 2. 顺序仲裁不可逆单向流
执行流水线严格遵循：
$$\text{Prepare (准备/认证)} \longrightarrow \text{Transform (变形/加密)} \longrightarrow \text{Sign (报文加签)} \longrightarrow \text{物理发包}$$
* 同一阶段内默认按 `.http` 文件的**书写声明顺序**保底执行；
* 支持使用 `priority=<int>`（默认 100，越小越靠前）进行精细权重微调。

#### 3. 纯函数式不可变补丁管道 (`PluginPatch`)
每个插件接收当前只读上下文，输出强类型 Patch，由内核完成原子合入并传递至下一阶段，杜绝数据竞争。

#### 4. 智能提示架构 (IntelliSense)
* **IDE 补全**：`rupost plugin schema` 导出全量 JSON Schema，VSCode/IntelliJ 键入 `@auth ` 时自动提示插件名与必填参数 Snippet；
* **TUI / Web 补全**：键入 `@` 字符原位弹出补全列表，Tab 展开参数占位符；
* **CLI 模板**：`rupost template <name>` 终端原位生成最佳实践代码块。

---

### 3.5 CLI 自定义命令双模动态路由与 TTY 透传 (Custom Commands SPI)

* **规范命名**：可执行文件命名为 `rupost-plugin-<name>`（推荐）或 `rupost-<name>`；
* **双模分发机制**：
  1. 用户运行 `rupost flamegraph snapshot.json --theme dark`；
  2. 若不是内置子命令，分发器向 `PluginRegistry` 查表寻址；
  3. 找到插件后，直接以 Tokio 异步子进程拉起，**完整继承终端 TTY**（原生支持交互式 TUI 与密码输入）；
  4. 自动注入上下文环境变量（`RUPOST_WORKSPACE`、`RUPOST_ENV`、`RUPOST_CONFIG_DIR`）；
  5. **子进程退出码 100% 穿透**，严格遵守 CI 门禁判定契约；
* **帮助大盘美学聚合**：运行 `rupost --help` 时，在末尾优雅展示 `EXTENSIONS` 区块及其单行说明。

---

### 3.6 AI 预留接口体系 (Context, Miner, Arbiter, MCP Bridge)

严格贯彻 `ai-master-roadmap.md` 中的宪法原则（B0 运行期零 AI、五阶段管线模板）：

```
+----------------------------------------------------------------------------------------------------+
|                                      统一预留的 4 大 AI SPI 接口                                    |
+----------------------------------------------------------------------------------------------------+
| 1. AiContextSPI  : 结构化上下文提取，强制经由 src/ai/redactor.rs 实施凭据安全脱敏清洗              |
| 2. AiMinerSPI    : 离线快照 (Snapshot v1.0) 根因归因挖掘器 (E2 就地归因 / E5 CI 聚类分诊)          |
| 3. EngineArbiter : 引擎裁决接口 ("AI建议，引擎裁决")，AST 严格校验丢弃非法建议与语法幻觉           |
| 4. McpToolBridge : MCP (Model Context Protocol) 工具双向暴露，充当 "AI Agent 的网络手脚"           |
+----------------------------------------------------------------------------------------------------+
```

---

### 3.7 优雅降级与环境自愈协议

1. **显式可选声明优先**：
   若脚本书写为 `@auth aws-sigv4 region=us-east-1 ?optional`，内核自动捕获缺失，输出警告并使用预留 Token 推进；
2. **全局忽略开关保障排障**：
   在现场排障时，传入 `--allow-missing-plugins`，所有缺失插件自动降级为警告跳过，基础发包与现场快照录制 100% 推进。

---

## 4. 风险、模糊点与关键技术难点 (Risks, Ambiguities & Technical Challenges)

### 4.1 阻断性风险 1：JSON 请求体重排篡改破坏签名 (Phase 0 决策 A)
* **风险**：旧代码无序反序列化并压缩 JSON，破坏了 AWS SigV4 / HMAC 等严格依赖 Body SHA256 的加签算法；
* **攻坚**：采用零分配语法探测，引入 `raw_body: Option<String>`，保证**发送的每一字节与加签的每一字节绝对一致**。

### 4.2 阻断性风险 2：现场快照静默吞没网络失败/超时请求
* **风险**：网络异常（DNS失败、超时）时 `r.response` 为 `None`，导致故障现场被丢弃；
* **攻坚**：重构快照模型为 `response: Option<...>` + 顶级 `error: Option<ExecutionErrorDetail>`，确保故障 100% 记录至数据黑匣子。

### 4.3 安全模糊点：项目级本地插件恶意执行 (RCE) 防范
* **策略**：命名严格受限于 `rupost-plugin-*`；工作区本地 `./.rupost/plugins` 默认必须携带 `--allow-project-plugins` 放行。

### 4.4 关键技术难点：大包管道并发死锁与插件冷启动时延隔离
* **防死锁**：必须采用 `tokio::join!` 异步双工并发读写，并在 `stdin` 写入完毕后显式发送 EOF；
* **时延隔离**：插件耗时单独记录为 `plugin_duration_us`，秒表严格与物理网络的 `ttfb` 和 `duration` 隔离。

### 4.5 跨平台路径与 CRLF 换行污染防护
* 所有路径传递统一经过 `src/utils/path_guard::normalize_posix_path`；
* 传递 Body 时保持原始字节，严禁隐式进行换行符转换。

---

## 5. 总结与路线图规划 (Roadmap & Milestones)

```
+----------------------------------------------------------------------------------------------------+
|                                    里程碑路线图 (Milestones Roadmap)                                |
+----------------------------------------------------------------------------------------------------+
| Phase 0: 基础设施保真与 AST 扩展 (Red-Green TDD)                                                    |
|   - Request JSON 零拷贝保真与 raw_body 保护                                                         |
|   - RequestMetadata 扩展语义化注解 (@auth, @transform, @sign, @pipeline, ?optional)                 |
|   - 现场快照模型重构 (100% 录制失败现场)                                                            |
+----------------------------------------------------------------------------------------------------+
| Phase 1: 注册表体系与生命周期管线调度 (Pipeline Engine)                                             |
|   - 实现 PluginRegistry 统一注册表与 ~/.rupost/registry.json 轻量缓存 (<5ms)                       |
|   - 实现 PluginPipeline: Stage 阶段切片与 Priority 仲裁排序                                         |
|   - 实现 StdioDriver: Tokio 异步双工、EOF 防死锁与 5s 熔断                                           |
|   - 声明式降级 (?optional 与 --allow-missing-plugins)                                               |
+----------------------------------------------------------------------------------------------------+
| Phase 2: CLI 自定义命令双模动态路由 (Custom Commands Engine)                                       |
|   - 未知子命令捕获与 rupost-plugin-* 动态路由寻址                                                   |
|   - TTY 独占直通透传与退出码 100% 穿透                                                              |
|   - rupost --help 优雅聚合 EXTENSIONS 列表                                                          |
|   - rupost plugin schema 导出与 IDE/TUI 智能提示联动                                                |
+----------------------------------------------------------------------------------------------------+
| Phase 3: AI SPI 预留与全场景离线产物挖掘验收 (AI Integration & E2E)                                 |
|   - 落地 AiContextSPI 脱敏清洗管道与 EngineArbiter 语法裁决器                                       |
|   - 编写示例插件: rupost-plugin-sigv4 (加签) 与 rupost-plugin-flamegraph (快照挖掘自定义命令)       |
|   - 断网即拷即走场景与 E2E 守护测试套件全绿验收                                                     |
+----------------------------------------------------------------------------------------------------+
```

本规划在恪守微内核自包含、单二进制即拷即走的前提下，将 DeepSeek Harness 注册表模式、生命周期顺序控制、CLI 动态子命令与 AI 预留接口融为一体，构建了极致优雅且具备无限扩展力的现代化 API 测试生态。
