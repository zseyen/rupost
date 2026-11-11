# 06 - 全场景插件体系 MVP 开发计划、用户画像与场景规约

- **文档代号**: `cross-scenario-plugins-mvp-plan`
- **归档目录**: `doc/plans/2026-11-10-cross-scenario-engine-and-plugins/`
- **状态**: Draft (v2.1.0 — 修订稿待评审；评审通过后回标 Execution Ready)
- **基线版本**: v2.1.0 (对抗性审查后修订)
- **权威声明**: **本文件（§3 范围裁决 + §4 里程碑）是 MVP 范围与施工顺序的唯一事实来源**；05 为 ADR 索引。
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
        │      ├─► 触发优雅降级: 终端输出黄色警告 [WARN]，插件不注入任何变更，
        │      │   请求按 .http 文件原样 Header 外发（引擎绝不注入魔法 Token，03 §2.1）
        │      ├─► 基础发包正常推进，绝不直接 Panic 阻断现场排障；结果标 DegradedPass
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
        ├─ 2. AI 智能根因分诊 (AI-as-Plugin 裁决，03 §7):
        │      rupost ai triage --snapshot incident.json [待建 M5]
        │      ├─► 内核强制凭据脱敏清洗 (redactor SSOT, Authorization: Bearer [REDACTED])
        │      ├─► 复用 crates/rupost-ai 归因能力 + 外部 AI 插件经注册表消费快照
        │      └─► "AI 建议，引擎裁决": AI 产出的语法产物必须过引擎 AST 校验后采纳
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
│ 2. DeepSeek Harness 统一注册表: 静态 manifest 发现 +   │ 2. @pipeline 动态语法糖       │
│    (name,stage) 复合键缓存与沙箱安全检查               │ 3. 在线插件应用市场 (Registry │
│ 3. 生命周期管线: Prepare -> Transform -> Sign 不可逆流 │    Center / 安装卸载 CLI)     │
│ 4. 不可变补丁模型: PluginPatch 纯函数式串联传递        │ 4. 运行时动态重载插件热更新   │
│ 5. Unix Stdio IPC 驱动: 异步双工读写、EOF、10MB 上限、  │ 5. MCP Server 双向协议全功能  │
│    5s 熔断、失败分类学、--strict-plugins 三态门禁      │    网关 (作为 Fast-follow)    │
│ 6. CLI 自定义命令: rupost <custom_cmd> 动态路由与 TTY  │ 6. RLIMIT 资源配额 / 全量     │
│ 7. 优雅降级机制: ?optional 与 --allow-missing-plugins  │    sha256 指纹 / plugin sync  │
│ 8. 内置加签宏最小集: $hmac_sha256, $sha256,            │    --regenerate / $md5 /      │
│    $random_int(min,max) 参数化 [01 §1.2 现状为零实现]  │    $random_string/$random_email│
│ 9. 快照 Schema v1.0 契约 (02 全量字段: 时序/错误/      │ 7. TUI/Web/CLI 模板智能提示   │
│    masking/演进策略) + --strict-plugins / --plugin-    │ 8. 注册表 miners 路由化调用   │
│    timeout / --plugin-env 旗标                         │ 9. 正则断言运算符             │
│ 10. AI-as-Plugin 契约: ai triage --snapshot 消费者     │ 10. EngineArbiter /           │
│    (复用 crates/rupost-ai) + AI 红线五条 (03 §7.2)     │    McpToolBridge (中期停泊)   │
└────────────────────────────────────────────────────────┴───────────────────────────────┘
```

> v2.1.0 修订：原第 8 项"AI 预留 SPI 接口契约定义与数据模型 (Trait + Struct)"废除（无消费者死代码，05 ADR-D9），替换为第 10 项 AI-as-Plugin 契约；新增第 8/9 项（内置宏与快照契约为全计划的前置地基，此前在里程碑中零 owner）。

---

## 4. MVP 详细开发计划与实施任务拆解 (Phased Execution Plan)

我们按照 **TDD 驱动开发 (Red-Green-Refactor)** 纪律，划分为 5 个原子化实施阶段：

```
+----------------------------------------------------------------------------------------------------+
|                                    MVP 5 阶段原子化开发路线图                                       |
+----------------------------------------------------------------------------------------------------+
| Milestone 1: 核心领域实体、AST 注解扩展与保真地基 (Phase 0: Domain & Parser)                        |
|   - 目标: 语义化注解语法 + 内置加签宏 ($hmac_sha256/$sha256) + 快照 Schema v1.0；                    |
|     出口 = 接口冻结清单 (PluginDirective/Stage/快照字段/单位制) + 二进制体积基线登记                |
+----------------------------------------------------------------------------------------------------+
| Milestone 2: DeepSeek Harness 统一注册表与静态 Manifest 引擎 (Phase 1: PluginRegistry)             |
|   - 目标: 静态 manifest 零子进程发现、(name,stage) 复合键、缓存安全硬化、stat 新鲜度闭环            |
+----------------------------------------------------------------------------------------------------+
| Milestone 3: 顺序仲裁管线、Stdio IPC 双工驱动器与失败分类学 (Phase 2: Pipeline & IPC Driver)        |
|   - 目标: 不可逆 FIFO 阶段流、三向排水防死锁、失败四分类降级联动、--strict-plugins 门禁             |
+----------------------------------------------------------------------------------------------------+
| Milestone 4: CLI 自定义命令双模动态路由 (Phase 3: Custom Commands Dispatcher)                      |
|   - 目标: args[0] 裁决序拦截、rupost-plugin-* 寻址、环境白名单、TOFU 横幅与信号感知退出码穿透       |
+----------------------------------------------------------------------------------------------------+
| Milestone 5: 智能提示 Schema 导出、AI 消费者与端到端全链路回归 (Phase 4: Schema & E2E Verification) |
|   - 目标: plugin schema 导出、ai triage --snapshot 消费者、Mock 插件实跑跨场景用例，回归全绿        |
+----------------------------------------------------------------------------------------------------+
```

---

### Milestone 1: 核心领域实体、AST 注解扩展与保真地基 (Phase 0: Domain & Parser)

* **主要职责**：让 `rupost` 原生识别 `@auth`、`@transform`、`@sign` 与 `?optional`，构建不可变变更实体，彻底剥离 `stage=` / `priority=` 等底层调度魔数；同时补齐两大前置地基——**内置加签宏**（01 §1.2 `[待建]`，现状零实现）与**快照 Schema v1.0**（02 §2，跨机黑匣子契约）。
* **开发任务拆解**：
  1. **Red (单测先行)**：
     - 在 `src/parser/metadata.rs` 对应测试模块中编写表驱动测试 `test_parse_plugin_directives_contract`；
     - 覆盖：`@auth aws-sigv4 region=us-east-1`、`@sign hmac-sha256 key=env.KEY ?optional`、带空格引号参数 `scope="read:orders write:orders"`、非法指令拦截；
     - 内置宏表驱动向量：`$hmac_sha256`/`$sha256` 已知答案测试（RFC 4231 向量）、`$random_int(min,max)` 边界；
     - 快照 Schema 契约测试：02 §2 结构 serde round-trip + 未知字段容忍（04 套件 3 前半）；
  2. **Green (实现领域模型)**：
     - 在 `src/plugin/types.rs` 与 `src/parser/types.rs` 中定义 `PluginDirective`、`PluginStage`（`Prepare`, `Transform`, `Sign`）实体；
     - 在 `RequestMetadata` 中扩展 `pub plugins: Vec<PluginDirective>`；
     - 在 `src/parser/metadata.rs` 中实现引号感知的 `@auth` / `@transform` / `@sign` 解析器并填充到 `RequestMetadata`；
     - **内置宏**：Cargo.toml 引入 `sha2`、`hmac`（纯 Rust 零系统依赖），在 `src/variable/resolver.rs` 实现 `$hmac_sha256(key, msg)`、`$sha256(msg)`，并将 `$random_int` 参数化为 `(min, max)`；
     - **快照模型扩展**：按 02 §2 重构 `src/history/model.rs` 的 `SnapshotSuite`/`SnapshotEntry` → `ExecutionSnapshotFile`/`ExecutionSnapshotEntry`（时序瀑布 `_us`、顶级 `error`、`masking`、`ExecutionOutcome`、演进策略），迁移 `src/history/storage.rs` 写入链路；快照落盘强制 `0600`；
  3. **Refactor & JJ 提交**：
     - 消除重复解析逻辑，执行 `cargo test -p rupost --lib parser` 确保单测全绿；
     - 使用 `jj describe` 提交。
* **出口条件（接口冻结清单）**：产出并提交《接口冻结清单》——`PluginDirective` 参数表示（有序对 + 引号感知）、`PluginStage` 三值枚举、`ExecutionSnapshotEntry` 字段集与单位制（`_us`）、`ExecutionOutcome` 枚举。后续里程碑变更需走独立评审（防止接口漂移击穿"原子化提交"，05 ADR）。
* **体积基线登记**：M1 开工前记录 release 二进制基线（当前实测 18.1 MiB），用于 DoD 增量验收。

---

### Milestone 2: DeepSeek Harness 统一注册表与静态 Manifest 引擎 (Phase 1: PluginRegistry)

* **主要职责**：建立插件统一索引、复合键 `(plugin_name, stage)` 无冲突映射、**静态 manifest 发现引擎**（03 §3.0 安全裁决）与轻量高速缓存机制。
* **开发任务拆解**：
  1. **Red (单测先行)**：
     - 编写测试用例 `test_plugin_manifest_serialization`、`test_registry_multi_hook_composite_key` 与 `test_registry_cache_staleness`；
     - 验证 Manifest JSON 序列化反序列化；验证同一插件注册多个阶段 Hook 不互相覆盖；验证基于文件 `mtime` + 前 4KB 哈希的高速缓存有效性判定；
     - **安全用例**：`test_discovery_spawns_no_processes`（发现路径零子进程派生）、路径穿越拒绝（`../` 与逃逸 symlink）、非属主缓存文件拒绝加载；
  2. **Green (实现注册表核心)**：
     - 在 `src/plugin/types.rs` 中定义 `PluginManifest`（含 `cache_scope`、`requires_env` 字段）、`CommandCapability`、`HookCapability`；
     - 在 `src/plugin/registry.rs` 中实现 `PluginRegistry`：
       - 使用 `(String, PluginStage)` 复合键索引 `hooks`，支持单插件跨阶段挂载；
       - 实现 `PluginRegistry::discover()`：**readdir + JSON 解析静态 manifest 文件（`rupost-plugin-<name>.json`），零子进程**；扫描 `$PATH` 与 `~/.rupost/plugins`；
       - 实现目录级 stat 新鲜度闭环：manifest 集合/指纹变化 → 条目失效重建（新装插件 sync 前即可被发现，03 §3.0）；
       - 实现并发安全缓存读写：通过 `registry.json.tmp.<pid>.<seq>` 原子重命名刷新 `~/.rupost/registry.json`（目标耗时 $\le 5\text{ms}$）；
       - 实现安全检查：工作区 `./.rupost/plugins` 必须有 `--allow-project-plugins` 放行；`~/.rupost/` 强制 `0700`/`registry.json` 强制 `0600` + 属主校验；可执行路径 canonicalize 白名单（**项目级 registry.json 文件废除**，03 §3.2）；
       - alias 冲突检测（项目级覆盖全局 + 启动 WARNING）；
  3. **Refactor & JJ 提交**：
     - 优化路径归一化，使用 `normalize_posix_path` 保护路径跨平台一致性；
     - 新增 Criterion bench `registry_cache_read`（口径：不含首轮目录扫描）。
* **出口条件（接口冻结清单）**：`PluginManifest` 字段集、`HookCapability.stage` 映射、`cache_scope` 枚举——M3 的 IPC 输入 JSON 与 M4 的路由键以此为准。

---

### Milestone 3: 顺序仲裁管线、Stdio IPC 双工驱动器与失败分类学 (Phase 2: Pipeline & IPC Driver)

* **主要职责**：确保插件严格按不可逆阶段 + 书写行序 (FIFO) 保序执行，通过 Stdio IPC 管道安全交互，防死锁防挂死；落成插件失败分类学与降级/门禁联动。
* **开发任务拆解**：
  1. **Red (单测先行)**：
     - **Mock 插件矩阵（04 §2.5，六件套）**：正常 Patch / 超 64KB 大报文 / 超 5s 超时 / 非法 JSON / `exit 3` / stderr 洪泛——fixture 用 `/bin/sh` + `dd`/`head` 或 tests/ 内编译 Rust 小二进制，**禁用 Python**（测试 harness 依赖解释器破坏跨平台 CI）；
     - 编写流水线调度单元测试，验证加签插件必然晚于加密插件执行，且同阶段内严格保持 `.http` 文件书写行序；
     - 失败分类学表驱动测试：`timeout/invalid_output/crashed/exit_non_zero` 四类判定与降级联动；
     - 正向对照组：真实 mock 插件注入特征 Header，防"降级成功 vs 插件根本没被调用"假守护（04 套件 2）；
  2. **Green (实现管道驱动与管线编排)**：
     - 在 `src/plugin/patch.rs` 中定义强类型不可变补丁 `PluginPatch`（支持 Header 大小写不敏感 Upsert 与 `remove_headers`）；
     - 新建模块 `src/plugin/driver.rs`（`StdioDriver`）：
       - 利用 `tokio::process::Command` 启动独立进程组子进程（`process_group(0)`）；
       - 拆分独立并发 Task 读 `stdin` 写入与 `stdout`/`stderr` 异步排水（10MB 载荷上限 + `take()` 限读；**stderr 截断 16KB 入 warnings**，CI 模式不直通终端）；
       - `stdin` 写完后显式 `shutdown().await` 并 drop 发送 EOF；
       - 配置 5 秒进程组 `SIGKILL` 超时熔断（`--plugin-timeout <sec>` 可覆盖）；
       - 隔离插件耗时至 `plugin_duration_us`，绝不污染物理网络 TTFB；
       - Hook 结果缓存：按 manifest `cache_scope` 复用 Patch（`request|suite`；`ttl` P2）；
     - 在 `src/plugin/pipeline.rs`（`PluginPipeline`）中实现管线编排：
       - 实现排序仲裁：按 `Stage`（`Prepare` $\to$ `Transform` $\to$ `Sign`）分组，同阶段内严格按声明书写行序（FIFO）执行；
       - 逐级应用 Patch 折叠合并到不可变请求上下文；
       - 遇到插件缺失**或四类失败**时，联动 `?optional`、`--allow-missing-plugins` 与 `--strict-plugins` 执行统一降级（03 §2.1：不注入 + 原样外发 + 三处 WARNING + `DegradedPass` + 降级计数汇总）或严格门禁拦截；
       - 含未解析 `{{var}}` 的兜底 Header → 跳过该请求并 WARNING；
  3. **Refactor & 接入 Middleware**：
     - 在 `src/runner/executor.rs` 中将 `PluginPipeline` 串入执行链路（含 `parallel` 模式与 fail-fast 组级 kill 传播测试）；
     - 使用 `jj` 原子化提交；
     - 新增 Criterion bench `plugin_overhead`（隔离验证插件耗时零污染 TTFB）；
     - 验收 04 套件 2 / 套件 5 / 负向矩阵全绿（owner 映射见 04 §4）。

---

### Milestone 4: CLI 自定义命令双模动态路由 (Phase 3: Custom Commands Dispatcher)

* **主要职责**：让 `rupost <custom_command>` 透明拉起外部插件，直通 TTY 并穿透退出码。
* **路由前提（03 §6.1 裁决序，M4 实现的唯一依据）**：根命令 `trailing_var_arg = true`（`src/cli/mod.rs:29`）使"未知子命令捕获"分支不存在——必须在 `src/cli/dispatch.rs` 对 `args[0]` **主动查表**：内置子命令 > 插件命令 > HTTP 方法名（大小写不敏感）> 报错 exit 2；插件命令与内置/方法名重名时安装期告警。
* **开发任务拆解**：
  1. **Red (单测先行)**：
     - 编写 CLI 派发测试，验证 `rupost <plugin_cmd>` 命中已注册的自定义插件命令；
     - **退出码穿透测试（v2.1.0 修正）**：真实子进程 `exit 7` → 主进程 exit 7（原值穿透）；子进程被信号杀死 → `128+signal`（废除 `unwrap_or(1)` 折叠与"127 或 1"错误断言——127 是 shell 找不到命令的码，直接 exec 拿不到）；registry 命中但二进制已删（缓存过期）→ exit 1 + 明确错误消息；
     - 环境白名单测试：子进程 env 仅含 `PATH/HOME/LANG/TERM/TMPDIR + RUPOST_*` + `requires_env` 声明项；
  2. **Green (实现拦截与动态路由)**：
     - 新建模块 `src/cli/commands/plugin_cmd.rs`；
     - 在 `src/cli/dispatch.rs` 中按 03 §6.1 裁决序实现 `args[0]` 拦截：
       - 向 `PluginRegistry` 查表寻址；
       - 命中后拉起子进程，直连 `std::process::Stdio::inherit()`（TTY 独占）+ **来源横幅（路径 + sha256 前 16 位）与首次 TOFU 确认**（03 §6.2）；
       - 注入上下文环境变量（`RUPOST_WORKSPACE`、`RUPOST_ENV`、`RUPOST_CONFIG_DIR`），**其余环境按白名单裁剪**（03 §6.2）；
       - 退出码信号感知穿透；
     - 在 `src/cli/mod.rs` 中优化 `--help` 输出，动态展示已安装的 `EXTENSIONS` 分组（**manifest 文本字段渲染前白名单净化**，03 §6.4）；
  3. **Refactor & JJ 提交**：
     - 确保与 CLAP Parser Rule 位置契约 100% 兼容（内联请求模式全局参数位置不变）；
     - 登记（不实现）：`template` 别名剥离与插件模板系统同批 `[P2]`（现状 `template` 是 `init` 别名，`src/cli/commands/mod.rs:58`）。

---

### Milestone 5: 智能提示 Schema 导出、AI 消费者与端到端全链路回归 (Phase 4: Schema & E2E)

* **主要职责**：产出智能提示 Schema 与首个内置 AI 消费者，编写完整端到端测试用例，全场景回归验收。
* **开发任务拆解**：
  1. **Schema 导出功能实现**：
     - 新增子命令 `rupost plugin schema`，聚合注册表中所有 Hook 的参数 Schema，输出标准 JSON Schema；
  2. **AI-as-Plugin 首个消费者（03 §7.2 / 05 ADR-D9）**：
     - `ai triage --snapshot <file>`：复用 `crates/rupost-ai::FailureAttributor` 适配 `SnapshotEntry`（`context_from_snapshot_entry` 适配器），B1/B2 降级用例（无 Key / AI 故障 / 输出非法）强制入测试方案；
     - AI 红线验收：脱敏先于 AI、`plugin_args` 永不入快照、`masking` 标记感知；
  3. **示例插件编写与跨场景验证**：
     - 在 `examples/plugins/` 下提供 2 个极简参考实现（附静态 manifest 文件）：
       - `rupost-plugin-aws-sigv4`（Python 实现的签名插件，产品参考实现）；
       - `rupost-plugin-flamegraph`（Shell/Rust 实现的时延火焰图挖掘命令）；
  4. **全场景防护网与全量回归**：
     - 新增集成测试 `tests/plugin_pipeline_test.rs` 与 `tests/custom_commands_test.rs`；验收 04 套件 1（standalone）与套件 3 录制保真半（owner 映射见 04 §4）；
     - 跑通完整三层校验：
       1. `cargo test -p rupost --lib`（全量单测 PASS）；
       2. `tests/verify_features.sh`（集成冒烟 PASS）；
       3. `examples/run_all.sh`（15 套复杂协议回归 100% PASS）；
  5. **进度与文档归档**：
     - 更新 `doc/plans/progress_summary.md`、`README.md` 与 `checkpoint.md`，使用 `jj describe` 闭环归档。

---

## 4.5 里程碑落地状态（2026-11-11 收官核对）

| Milestone | 状态 | 交付证据 |
| :-- | :--- | :--- |
| M1 | ✅ 落地 | 内置宏（RFC 4231 向量）/ 快照 Schema v1.0 / FIFO 仲裁 / 冻结清单 interface-freeze-m1.md |
| M2 | ✅ 落地 | 静态 manifest 零子进程发现 / 缓存安全硬化 / bench 0.58ms ≤5ms |
| M3 | ✅ 落地 | StdioDriver / 失败分类学 / 统一降级 / `--strict-plugins`（04 套件 2/5 全绿）|
| M4 | ✅ 落地 | args[0] 裁决序 / TOFU / 环境白名单 / 信号感知穿透（e2e exit 7）|
| M5 | ✅ 落地 | plugin list/schema / `ai triage-snapshot`（masking 感知）/ 套件 1 与套件 4 |
| M6 | 🆕 立项（v2.1.1） | 插件运行时就绪：`runtime` 声明 + readdir 预检 + 三态状态机 + 处方提示（03 §3.1.1/§8.3）+ §6 缺口收尾清单 |

**M6 任务拆解（环境就绪与缺口收尾）**：
1. **runtime 预检与三态**：`PluginRuntime` manifest 解析；readdir 级解释器存在性预检；插件三态（ready/pending/failed）状态机；`plugin list` 三态可视化与处方提示（03 §3.1.1）；
2. **失败分类学扩展**：`runtime_missing` / `dependency_missing`（stderr 特征表驱动：`ModuleNotFoundError`/`Cannot find module`/`ImportError`，从 stderr 提取缺失模块名入处方）；
3. **`requires_env` 放行告警**：manifest 声明但运行时缺 `--plugin-env` 放行 → 启动 WARNING（不阻断）；
4. **`--max-snapshot-body-bytes` 旗标接线**（冻结清单缺口 #4）；
5. **exit 2 = AST 语法错误归位**（P1-5 残尾：dispatch 未知命令已 exit 2，AST 解析错误路径仍 exit 1）；
6. **DoD 性能验证测试补齐**：时延隔离确定性不变量（mock 插件 sleep 3s 断言 `ttfb < 100ms && plugin_duration_us > 3s`）；suite `cache_scope` 进程数上限（100 请求 × 1 @auth ≤ 2）；fail-fast × 插件组级 kill 传播；并发模式 × 插件管线组合；
7. **04 套件 3 落地**：`tests/artifact_mining_test.rs`（Schema 契约半已在 M1 交付于 lib 内，录制保真半补集成测试文件）；
8. **掩码扩面表驱动单测**：X-Sign 类自定义签名头 / query token / Set-Cookie / body PII 全表（02 §4）。

**登记不做的裁决项（P2/生态期，非缺口）**：`@pipeline`/`@use` 语法、miners 路由化入口、TUI/Web 补全、`$md5`/`$random_string`/`$random_email`、正则断言、`plugin sync --regenerate`、doctor 自报健康协议（生态期，`plugin list --doctor` 聚合）、降级链 `fallback.builtin`（仅签名类开放）、RLIMIT 配额、全量 sha256 指纹、MCP Server、Windows shebang 透明代理（跨平台 CI 批次）。

---

## 5. MVP 验收标准与交付物契约 (DoD & Acceptance Criteria)

> v2.1.0 修订：指标全部改为**可证伪**口径（v2.0.0 的"≤100KB 增量"偏紧、"TTFB 误差为 0"无基准量、"564+ 测试"陈旧、"内存 ≤30MB"无测量手段）。

| 检验维度 | 验收标准 (Definition of Done) | 验证手段 |
| :--- | :--- | :--- |
| **功能闭环** | 1. 成功解析 `@auth`、`@transform`、`@sign` 与 `?optional`。<br>2. 严格按 `Prepare` $\to$ `Transform` $\to$ `Sign` 阶段顺序执行。<br>3. `rupost <plugin_cmd>` 可正常拉起外部工具并透传退出码（含信号场景 `128+signal`）。<br>4. 内置宏 `$hmac_sha256`/`$sha256`/`$random_int(min,max)` 全部可用（RFC 已知答案向量）。 | 自动化集成测试套件 |
| **性能基线** | 1. release 二进制相对 M1 基线增量 $\le 500\text{KB}$（含 sha2/hmac 引入）。<br>2. 注册表缓存读取 $\le 5\text{ms}$（Criterion `registry_cache_read`，口径不含首轮目录扫描）。<br>3. **时延隔离确定性不变量**：mock 插件 sleep 3s 场景下断言 `ttfb < 100ms` 且 `plugin_duration_us > 3s`（Criterion `plugin_overhead`）。<br>4. suite 级 `cache_scope` 验收：100 请求 × 1 `@auth` 套件，插件子进程总数 $\le 2$。 | Criterion Benchmark 基准压测 |
| **稳定性与安全** | 1. 插件遭遇死循环或大报文时，超时窗内确定性进程组 `SIGKILL` 熔断，绝无挂死（含 stderr 洪泛场景）。<br>2. 工作区本地插件未显式放行时严禁执行 (RCE 防护)；发现路径零子进程派生；registry 路径穿越/属主攻击用例全拒。 | 压力注入与超时熔断测试 |
| **自愈降级** | 缺失插件的用例在 `--allow-missing-plugins` 下：请求按文件原样 Header 外发 + 三处 WARNING + 快照 `DegradedPass` + 降级计数汇总；`--strict-plugins` 下整体 exit 1。 | 跨机器离线快照录制测试 |
| **快照契约** | 02 §2 全字段 serde round-trip + 未知字段容忍；落盘权限 `0600`；`plugin_args` 不入快照（审计测试）。 | Schema 契约测试 |
| **代码工程质量** | 1. `cargo clippy --workspace --all-targets -- -D warnings` 零告警。<br>2. `cargo fmt --check` 零漂移。<br>3. **测试基线不回退**（M1 前记录基线，当前 950+ 测试属性）+ 04 全部套件与 15 套 E2E 场景 100% PASS。 | CI 自动化静态门禁 |
