# 06 - 全场景插件体系 MVP 开发计划、用户画像与场景规约

- **文档代号**: `cross-scenario-plugins-mvp-plan`
- **归档目录**: `doc/plans/2026-11-10-cross-scenario-engine-and-plugins/`
- **基线版本**: v1.0.0 (Execution Ready)
- **关联设计**:
  - [`01-core-capabilities-and-api.md`](./01-core-capabilities-and-api.md)
  - [`02-artifact-contract-for-plugins.md`](./02-artifact-contract-for-plugins.md)
  - [`03-plugin-spi-and-degradation.md`](./03-plugin-spi-and-degradation.md)
  - [`04-testing-strategy-and-scenarios.md`](./04-testing-strategy-and-scenarios.md)
  - [`05-comprehensive-architecture-and-requirements-summary.md`](./05-comprehensive-architecture-and-requirements-summary.md)
  - [`ai-master-roadmap.md`](../archive/2026-q4/2026-10-27-ai-master-roadmap.md)

---

## 1. 深度用户画像分析 (User Personas)

为了确保设计不沦为空泛的技术自嗨，我们深入现代软件工程协作的真实第一现场，刻画出 4 类典型角色画像：

```
┌────────────────────────────────────────────────────────────────────────────────────────┐
│                                   四维核心用户画像矩阵                                 │
├────────────────────────────┬─────────────────────────────┬─────────────────────────────┤
│ 1. 张晨 (Chen Zhang)       │ 2. 李浩 (Hao Li)            │ 3. 王建国 (Jianguo Wang)    │
│    资深微服务/API 开发     │    DevOps / CI/CD 架构师    │    SRE 运维总监 / 现场排障  │
│    关注: 鉴权加签/智能提示 │    关注: 0外部依赖/确定性门禁│    关注: 即拷即走/数据黑匣子│
├────────────────────────────┴─────────────────────────────┴─────────────────────────────┤
│ 4. 赵梓涵 (Zihan Zhao)                                                                 │
│    研发效能专家 / 内部插件作者                                                         │
│    关注: DeepSeek Harness 统一注册表 / 外部语言自由编写 / MCP Agent 无缝集成            │
└────────────────────────────────────────────────────────────────────────────────────────┘
```

### 1.1 用户画像 1：张晨 (Chen Zhang) —— 资深微服务与 API 开发者
* **背景与定位**：负责核心业务微服务群研发，接口涉及 AWS SigV4、自研动态 Token 与 AES-GCM 载荷加密。
* **痛点与诉求**：
  * 过去用 Postman 时，必须在 Pre-request Script 里写大量难以维护的 JavaScript 脚本；
  * 希望在纯文本 `.http` 文件中**以自然语义写测试**，键入 `@auth ` 时编辑器能精准弹出补全列表，并自动填充必填参数模板；
  * 对时序敏感，要求系统严密保障“先获取 Token，再做载荷加密，最后执行报文加签”，严禁因执行倒序导致签名失效。
* **价值交付**：语义化一等公民语法（`@auth`、`@transform`、`@sign`）、IDE JSON Schema 智能提示与不可逆单向阶段执行流。

### 1.2 用户画像 2：李浩 (Hao Li) —— DevOps 与 CI/CD 平台架构师
* **背景与定位**：维护大型 Kubernetes 集群流水线，日均触发数万次自动化回归构建。
* **痛点与诉求**：
  * 构建容器为极简的 Alpine 或 Debian-Slim 镜像，最怕工具要求宿主安装 Python、Node 或动态 C 库（`glibc` 依赖地狱）；
  * 最怕测试工具在发生异常时挂死（双向死锁），导致 CI 流水线超时占死 Runner 资源；
  * 需要标准的 Linux Exit Code（0=全部通过，1=断言失败，2=语法错误）与工业级 JUnit XML 报告。
* **价值交付**：纯静态单二进制（0 外部运行时依赖）、纯 Rust 内置宏（`$sha256`、`$hmac_sha256`）、Stdio 管道 5 秒硬熔断机制与合规 JUnit XML。

### 1.3 用户画像 3：王建国 (Jianguo Wang) —— SRE 运维总监 / 应急排障专家
* **背景与定位**：专职生产事故应急响应，常年通过堡垒机登录物理隔离、无外网访问的金融/风控核心服务器。
* **痛点与诉求**：
  * 堡垒机环境极其简陋，严禁安装外部第三方软件，遇到生产接口 504 报警需要即时排查；
  * 如果从研发机拷贝过来的测试用例因为写了 `@auth aws-sigv4` 就直接 Panic 崩溃，排障工作将彻底瘫痪；
  * 现场无法运行重型分析插件，必须能够将包含**物理网络微秒级时序（DNS/TCP/TLS/TTFB）与全量原始报文**的黑匣子无损录制下来，以便拷回开发机进行深度复盘。
* **价值交付**：单二进制“即拷即走 (Copy & Run)”、`--allow-missing-plugins` 声明式自愈降级与 `snapshot.json` 数据黑匣子保真录制。

### 1.4 用户画像 4：赵梓涵 (Zihan Zhao) —— 研发效能架构师 / 插件生态作者
* **背景与定位**：负责企业内部研发生效工具链建设，需要为全公司定制私有的验签算法、时延火焰图工具与 AI 归因助手。
* **痛点与诉求**：
  * 不希望为了定制功能而 Fork `rupost` 源码，希望用现有的 Python/Go 脚本低成本扩展；
  * 借鉴了 DeepSeek Harness 的统一注册表设计，希望插件既能在请求执行期作为钩子运行，又能作为独立的 CLI 子命令（如 `rupost flamegraph`）对外暴露；
  * 未来规划接入 AI Agent，希望插件能力能原生暴露为标准 MCP (Model Context Protocol) Tools。
* **价值交付**：方案 D Unix Stdio IPC 管道协议、`PluginRegistry` 统一注册表、双模 TTY 独占直通与 MCP 接口桥梁。

---

## 2. 四大真实端到端场景说明 (Scenarios Walkthrough)

### 场景 A：日常敏捷开发与智能提示场景 (Full Dev)

```
[ 开发者在 IDE 中打开 orders.http ]
        │
        ├─ 1. 键入 "@" 字符
        │      └─► IDE 触发 JSON Schema 自动补全，弹出 @auth / @transform / @sign 候选列表
        │
        ├─ 2. 选中 "@auth aws-sigv4"
        │      └─► 自动展开参数 Snippet: region=us-east-1 service=execute-api
        │
        ├─ 3. 执行测试: rupost test orders.http
        │      ├─► Prepare 阶段: 调用 rupost-plugin-aws-sigv4 获取动态 STS 会话
        │      ├─► Transform 阶段: 自动对 Body 执行 AES 加密
        │      ├─► Sign 阶段: 计算最终加签报文
        │      └─► Reqwest 发送物理请求并实时渲染 HSL 终端高审美彩色结果
```

### 场景 B：云原生流水线零依赖门禁场景 (Pipeline CI/CD)

```
[ GitHub Actions / GitLab CI 容器启动 (Alpine Linux, 纯静态单二进制) ]
        │
        ├─ 1. 触发回归: rupost test ./tests/ --report-file junit.xml --fail-fast
        │      ├─► 0 外部运行时: 不依赖 Python/Node，完全由纯 Rust 内置宏 ($hmac_sha256) 完成加签
        │      ├─► 确定性 DAG: 并发驱动测试用例拓扑依赖
        │      └─► @teardown 清理: 无论测试成功或断言失败，100% 逆拓扑清理环境脏数据
        │
        ├─ 2. 判定与产物归档:
        │      ├─► 生成合规 junit.xml (经过 strip-ansi-escapes 过滤，机器解析 0 故障)
        │      └─► 抛出明确 Exit Code (0=PASS, 1=FAIL)，精准卡控流水线发布门禁
```

### 场景 C：生产堡垒机即拷即走与自愈录制 (Portable Debug)

```
[ 生产机房突发 504 报警，SRE 王建国登录受限跳板机 (无 Python / 断网) ]
        │
        ├─ 1. scp 拷贝单二进制: scp ./rupost jumper:/tmp/rupost
        │
        ├─ 2. 执行排查: 
        │      rupost test debug_suite.http --allow-missing-plugins --save-snapshot /tmp/incident.json
        │
        ├─ 3. 内核自愈降级:
        │      ├─► 发现用例含 @auth aws-sigv4，但环境缺失 Python 插件
        │      ├─► 触发优雅降级: 终端输出黄色警告 [WARN]，自动注入预留的临时运维 Header
        │      ├─► 基础发包正常推进，绝不直接 Panic 阻断现场排障
        │      └─► 发生连接超时后，内核 100% 将物理底层时延 (DNS: 12us, TCP: 5000ms超时) 完整写入 incident.json
```

### 场景 D：跨环境数据黑匣子离线深度挖掘 (Offline Deep Mining)

```
[ SRE 将现场生成的 incident.json 拷回本地研发机 ]
        │
        ├─ 1. 终端时延火焰图可视化:
        │      rupost flamegraph incident.json --theme dark
        │      └─► CLI 动态路由至 rupost-plugin-flamegraph，独占 TTY 渲染交互式瀑布图
        │
        ├─ 2. AI 智能根因分诊:
        │      rupost ai triage incident.json
        │      ├─► AiContextSPI: 内核强制进行凭据脱敏清洗 (Authorization: Bearer [REDACTED])
        │      ├─► 插件调用 LLM 推理: "发现 TCP 握手耗时达 5000ms，定位为内网网关 Envoy 路由死锁"
        │      └─► EngineArbiter: 校验 AI 建议的修复补丁合法性，确认无误后提示采纳
```

---

## 3. 全场景插件体系 MVP 范围界定 (MVP Inclusions vs Non-Goals)

遵循 **MVP 原则 (Minimum Viable Product)** 与 **KISS 原则**，我们严格划定 MVP 版本的交付边界：

```
┌────────────────────────────────────────────────────────────────────────────────────────┐
│                              MVP 范围裁决与阶段交付界限                                │
├────────────────────────────────────────────────────────┬───────────────────────────────┤
│ 核心纳入 (In Scope - MVP 必须交付)                     │ 后续迭代 (Out of Scope / P2+) │
├────────────────────────────────────────────────────────┼───────────────────────────────┤
│ 1. 领域 AST 扩展: @auth, @transform, @sign, ?optional  │ 1. 复杂多行 @use 缩进块语法   │
│ 2. DeepSeek Harness 统一注册表: PluginRegistry 与缓存   │ 2. @pipeline 动态语法糖       │
│ 3. 生命周期管线: Prepare -> Transform -> Sign 不可逆流 │ 3. 在线插件应用市场 (Registry │
│ 4. 不可变补丁模型: PluginPatch 纯函数式串联传递        │    Center / 安装卸载 CLI)     │
│ 5. Unix Stdio IPC 驱动: 异步双工读写、EOF 与 5s 熔断   │ 4. 运行时动态重载插件热更新   │
│ 6. CLI 自定义命令: rupost <custom_cmd> 动态路由与 TTY  │ 5. MCP Server 双向协议全功能  │
│ 7. 优雅降级机制: ?optional 与 --allow-missing-plugins  │    网关 (作为 Fast-follow)    │
│ 8. AI 预留 SPI 接口契约定义与数据模型 (Trait + Struct)  │                               │
└────────────────────────────────────────────────────────┴───────────────────────────────┘
```

---

## 4. MVP 详细开发计划与实施任务拆解 (Phased Execution Plan)

我们按照 **TDD 驱动开发 (Red-Green-Refactor)** 纪律，划分为 5 个原子化实施阶段：

```
+----------------------------------------------------------------------------------------------------+
|                                    MVP 5 阶段原子化开发路线图                                       |
+----------------------------------------------------------------------------------------------------+
| Milestone 1: 核心领域实体与 AST 注解扩展 (Phase 0: Domain & Parser)                                |
|   - 目标: 锁定语义化注解语法，完成不可变补丁与阶段枚举定义，测试先行 (100% 覆盖)                     |
+----------------------------------------------------------------------------------------------------+
| Milestone 2: DeepSeek Harness 统一注册表与 Manifest 引擎 (Phase 1: PluginRegistry)                 |
|   - 目标: 建立插件元数据模型、扫描算法、~/.rupost/registry.json 轻量缓存与沙箱安全检查              |
+----------------------------------------------------------------------------------------------------+
| Milestone 3: 顺序仲裁管线与 Stdio IPC 双工驱动器 (Phase 2: Pipeline & IPC Driver)                  |
|   - 目标: 实现不可逆单向阶段流调度，防死锁双工读写，5 秒硬超时熔断与自愈降级处理                     |
+----------------------------------------------------------------------------------------------------+
| Milestone 4: CLI 自定义命令双模动态路由 (Phase 3: Custom Commands Dispatcher)                      |
|   - 目标: 未知子命令捕获、rupst-plugin-* 寻址、TTY 独占直通透传与退出码 100% 穿透                   |
+----------------------------------------------------------------------------------------------------+
| Milestone 5: 智能提示 Schema 导出与端到端全链路回归 (Phase 4: Schema & E2E Verification)           |
|   - 目标: rupost plugin schema 导出，编写 Mock 插件实跑跨场景用例，回归 examples 与 tests 全绿     |
+----------------------------------------------------------------------------------------------------+
```

---

### Milestone 1: 核心领域实体与 AST 注解扩展 (Phase 0: Domain & Parser)

* **主要职责**：让 `rupost` 原生识别 `@auth`、`@transform`、`@sign` 与 `?optional`，构建不可变变更实体。
* **开发任务拆解**：
  1. **Red (单测先行)**：
     - 在 `src/parser/metadata.rs` 对应测试模块中编写表驱动测试 `test_parse_plugin_directives_contract`；
     - 覆盖：`@auth aws-sigv4 region=us-east-1`、`@sign hmac-sha256 key=env.KEY ?optional`、非法指令拦截；
  2. **Green (实现领域模型)**：
     - 在 `src/parser/types.rs` 中新增 `PluginDirective`、`PluginStage`（`Prepare`, `Transform`, `Sign`）实体；
     - 在 `RequestMetadata` 中扩展 `pub plugins: Vec<PluginDirective>`；
     - 在 `src/parser/metadata.rs` 中实现 `@auth` / `@transform` / `@sign` 解析并填充到 `RequestMetadata`；
  3. **Refactor & JJ 提交**：
     - 消除重复解析逻辑，执行 `cargo test -p rupost --lib parser` 确保单测全绿；
     - 使用 `jj describe` 提交。

---

### Milestone 2: DeepSeek Harness 统一注册表与 Manifest 引擎 (Phase 1: PluginRegistry)

* **主要职责**：建立插件统一索引、元数据自省模型与轻量高速缓存机制。
* **开发任务拆解**：
  1. **Red (单测先行)**：
     - 编写测试用例 `test_plugin_manifest_serialization` 与 `test_registry_cache_staleness`；
     - 验证 Manifest JSON 序列化反序列化；验证基于文件 `mtime` 的高速缓存有效性判定；
  2. **Green (实现注册表核心)**：
     - 新建模块 `src/runner/engine/plugin/manifest.rs`：定义 `PluginManifest`、`CommandCapability`、`HookCapability`；
     - 新建模块 `src/runner/engine/plugin/registry.rs`：
       - 实现 `PluginRegistry::discover()`：扫描 `$PATH` 与 `~/.rupost/plugins`；
       - 实现缓存读写：读取/刷新 `~/.rupost/registry.json`（目标耗时 $\le 5\text{ms}$）；
       - 实现安全检查：工作区 `./.rupost/plugins` 必须有 `--allow-project-plugins` 放行；
  3. **Refactor & JJ 提交**：
     - 优化路径归一化，使用 `normalize_posix_path` 保护路径跨平台一致性。

---

### Milestone 3: 顺序仲裁管线与 Stdio IPC 双工驱动器 (Phase 2: Pipeline & IPC Driver)

* **主要职责**：确保插件严格按不可逆阶段保序执行，通过 Stdio IPC 管道安全交互，防死锁防挂死。
* **开发任务拆解**：
  1. **Red (单测先行)**：
     - 编写 Mock 插件脚本（分别模拟正常输出 Patch、写入超 64KB 大报文、运行超过 5s 超时）；
     - 编写流水线调度单元测试，验证加签插件必然晚于加密插件执行；
  2. **Green (实现管道驱动与管线编排)**：
     - 新建模块 `src/runner/engine/plugin/patch.rs`：定义强类型不可变补丁 `PluginPatch`；
     - 新建模块 `src/runner/engine/plugin/driver.rs`（`StdioDriver`）：
       - 利用 `tokio::process::Command` 启动子进程；
       - 使用 `tokio::join!` 并发读写 `stdin` 与 `stdout`；
       - `stdin` 写完后显式 `shutdown()` 并 drop 发送 EOF；
       - 配置 5 秒 `kill_on_drop(true)` 超时熔断；
       - 隔离插件耗时至 `plugin_duration_us`，绝不污染物理网络 TTFB；
     - 新建模块 `src/runner/engine/plugin/pipeline.rs`（`PluginPipeline`）：
       - 实现排序仲裁：按 `Stage`（`Prepare` $\to$ `Transform` $\to$ `Sign`）分组，同阶段内按声明顺序 + `priority` 排序；
       - 逐级应用 Patch 折叠合并到不可变请求上下文；
       - 遇到插件缺失时，联动 `?optional` 与 `--allow-missing-plugins` 执行自愈降级；
  3. **Refactor & 接入 Middleware**：
     - 在 `src/runner/executor.rs` 中将 `PluginPipeline` 串入执行链路；
     - 使用 `jj` 原子化提交。

---

### Milestone 4: CLI 自定义命令双模动态路由 (Phase 3: Custom Commands Dispatcher)

* **主要职责**：让 `rupost <custom_command>` 透明拉起外部插件，直通 TTY 并穿透退出码。
* **开发任务拆解**：
  1. **Red (单测先行)**：
     - 编写 CLI 派发测试，验证 `rupost unknown_cmd` 能够命中已注册的自定义插件命令；
     - 验证子进程退出码为 127 或 1 时，主进程完全匹配该退出码；
  2. **Green (实现未知命令捕获与动态路由)**：
     - 新建模块 `src/cli/commands/plugin_cmd.rs`；
     - 在 `src/cli/dispatch.rs` 中增加未匹配命令的兜底分支：
       - 向 `PluginRegistry` 查表寻址；
       - 命中后拉起子进程，直连 `std::process::Stdio::inherit()`（TTY 独占）；
       - 注入环境变量（`RUPOST_WORKSPACE`、`RUPOST_ENV`、`RUPOST_CONFIG_DIR`）；
       - `status.code().unwrap_or(1)` 退出；
     - 在 `src/cli/mod.rs` 中优化 `--help` 输出，动态展示已安装的 `EXTENSIONS` 分组；
  3. **Refactor & JJ 提交**：
     - 确保与 CLAP Parser Rule 位置契约 100% 兼容。

---

### Milestone 5: 智能提示 Schema 导出与端到端全链路回归 (Phase 4: Schema & E2E)

* **主要职责**：产出智能提示 Schema，编写完整端到端测试用例，全场景回归验收。
* **开发任务拆解**：
  1. **Schema 导出功能实现**：
     - 新增子命令 `rupost plugin schema`，聚合注册表中所有 Hook 的参数 Schema，输出标准 JSON Schema；
  2. **示例插件编写与跨场景验证**：
     - 在 `examples/plugins/` 下提供 2 个极简参考实现：
       - `rupost-plugin-aws-sigv4`（Python 实现的签名插件）；
       - `rupost-plugin-flamegraph`（Shell/Rust 实现的时延火焰图挖掘命令）；
  3. **全场景防护网与全量回归**：
     - 新增集成测试 `tests/plugin_pipeline_test.rs` 与 `tests/custom_commands_test.rs`；
     - 跑通完整三层校验：
       1. `cargo test -p rupost --lib`（全量单测 PASS）；
       2. `tests/verify_features.sh`（集成冒烟 PASS）；
       3. `examples/run_all.sh`（15 套复杂协议回归 100% PASS）；
  4. **进度与文档归档**：
     - 更新 `doc/plans/progress_summary.md`、`README.md` 与 `checkpoint.md`，使用 `jj describe` 闭环归档。

---

## 5. MVP 验收标准与交付物契约 (DoD & Acceptance Criteria)

| 检验维度 | 验收标准 (Definition of Done) | 验证手段 |
| :--- | :--- | :--- |
| **功能闭环** | 1. 成功解析 `@auth`、`@transform`、`@sign` 与 `?optional`。<br>2. 严格按 `Prepare` $\to$ `Transform` $\to$ `Sign` 阶段顺序执行。<br>3. `rupost <plugin_cmd>` 可正常拉起外部工具并透传退出码。 | 自动化集成测试套件 |
| **性能基线** | 1. 静态二进制增加 $\le 100\text{KB}$。<br>2. 注册表缓存读取耗时 $\le 5\text{ms}$。<br>3. 插件冷启动耗时独立统计，物理网络 TTFB 误差为 0。 | Criterion Benchmark 基准压测 |
| **稳定性与安全** | 1. 插件遭遇死循环或大报文时，5 秒内确定性 `SIGKILL` 熔断，绝无挂死。<br>2. 工作区本地插件未显式放行时严禁静默执行 (RCE 防护)。 | 压力注入与超时熔断测试 |
| **自愈降级** | 缺失插件的用例在 `--allow-missing-plugins` 标志下，100% 输出黄色 WARN 并发包，完整录入快照。 | 跨机器离线快照录制测试 |
| **代码工程质量** | 1. `cargo clippy --all-targets -- -D warnings` 零告警。<br>2. `cargo fmt --check` 零漂移。<br>3. 全库 564+ 单元测试与 15 套 E2E 场景 100% PASS。 | CI 自动化静态门禁 |
