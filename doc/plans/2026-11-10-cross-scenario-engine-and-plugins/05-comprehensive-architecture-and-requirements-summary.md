# 05 - 全场景自适应架构：技术决策记录索引 (ADR Index)

- **文档代号**: `cross-scenario-engine-and-plugins-summary`
- **归档目录**: `doc/plans/2026-11-10-cross-scenario-engine-and-plugins/`
- **状态**: Draft (v2.1.0 — 评审通过后回标 Approved)
- **基线版本**: v2.1.0 (ADR Index — 对抗性审查后从"白皮书"降级重构)
- **文档定位裁决（v2.1.0）**: 本文件**不再是权威规范**。v2.0.0 的白皮书形态造成与 01/03/06 大面积复述漂移（需求复述三份、路线图两套互斥）。现降级为**决策记录（ADR）与风险登记索引**：
  * **范围唯一权威**：[06 §3](./06-mvp-implementation-plan-personas-and-scenarios.md)；
  * **快照契约唯一权威**：[02](./02-artifact-contract-for-plugins.md)；
  * **插件发现/IPC/降级/仲裁唯一权威**：[03](./03-plugin-spi-and-degradation.md)；
  * **基础能力与 API 基线**：[01](./01-core-capabilities-and-api.md)；
  * **测试防护网**：[04](./04-testing-strategy-and-scenarios.md)。

---

## 1. 决策记录 (Architecture Decision Records)

| ID | 决策 | 内容摘要 | 详细规范 |
| :-- | :--- | :--- | :--- |
| **D1** | 插件机制选型方案 D | Unix Stdio IPC 进程管道战胜 Wasm 沙箱 / dylib / 嵌入式脚本：零二进制膨胀、任意语言插件、进程级故障隔离、"缺插件即降级"哲学与三大场景契合。对比：Wasmtime 膨胀 20~35MB 且交叉编译脆；dylib ABI 跨平台灾难且可 segfault 主进程；嵌入式脚本无法复用既有 Python/Node SDK。先例：Git Filter Process、GitHub CLI extension | §2.1/§2.2（保留原文档） |
| **D2** | 发现即静态 Manifest（v2.1.0 安全裁决） | 废除"探测时执行 `--manifest` 自省"协议——它使扫描变成代码执行点（npm/pip postinstall 向 `$PATH` 丢 `rupost-plugin-*` 即 RCE）。发现 = readdir + JSON 解析零子进程；`--manifest` 仅保留给显式 `plugin sync --regenerate` `[P2]` | [03 §3.0/§3.1] |
| **D3** | 注册表缓存与信任硬化 | registry.json = 可重建索引；`0700`/`0600` + 属主校验；可执行路径 canonicalize 白名单（拒绝 `..`/逃逸 symlink）；**项目级 `./.rupost/registry.json` 文件废除**（防恶意仓库绕闸）；mtime + 前 4KB 哈希指纹 + 原子重命名写保护；`$PATH` 不再无条件"全局受信任" | [03 §3.2、§10.3] |
| **D4** | 子进程环境白名单 | 插件子进程默认仅继承 `PATH/HOME/LANG/TERM/TMPDIR + RUPOST_*`；manifest `requires_env` 声明 + `--plugin-env` 显式放行；全量环境继承废除（防堡垒机凭据经第三方插件外泄） | [03 §6.2] |
| **D5** | 降级语义唯一化（不静默、不注入魔法值） | 缺失/失败统一 = 插件不注入 + 请求按 `.http` 原样外发 + 三处 WARNING + 快照 `DegradedPass` + 降级计数汇总；含未解析 `{{var}}` 的兜底 Header 视为兜底缺失 → 跳过该请求。**否决**"引擎注入预留 Token"（魔法值无来源）；**否决**"静默降级"（反静默哲学）。残余风险（未签名请求发往生产的行为差分）由 `--strict-plugins` CI 硬门禁兜底 | [03 §2、§10.5] |
| **D6** | 执行仲裁：阶段不可逆 + 同阶段纯 FIFO | 彻底废除 `stage=`/`priority=` 魔数；`@pipeline` 标 `[P2]` 且与阶段序冲突 = 语法错误 exit 2；三态判定 `ExecutionOutcome` | [03 §5] |
| **D7** | CLI 双模路由裁决序 | 根命令 `trailing_var_arg` 使"未知子命令捕获"分支不存在 → 主动拦截 `args[0]` 查表：内置 > 插件 > HTTP 方法名，否则 exit 2；退出码信号感知穿透（`None → 128+signal`）；TTY 直通 + 来源横幅 + 首次 TOFU 确认；hook 禁 TTY | [03 §6] |
| **D8** | 快照契约单源 + 演进策略 | 02 为 Schema 唯一权威（v2.0.0 中 02/05/04 三方矛盾已收敛）：`entries` 定名、顶级 `error` 字段、四个 payload 类型补全、`masking` 标记、字段只增不改 + 未知字段容忍 + 破坏性变更递增版本；快照 `0600`；`plugin_args` 永不入快照 | [02 §2] |
| **D9** | AI-as-Plugin（废除内核 AI trait） | 不定义 `AiContextProvider`/`AiSnapshotMiner`/`EngineArbiter`/`McpToolBridge` 四个内核 trait（无消费者死代码）。AI 扩展机制 = 插件方案 D 本身：内核只承诺快照契约 + 脱敏 SSOT（`src/ai/redactor.rs`）+ 首个内置消费者 `ai triage --snapshot` `[待建 M5]`（复用 `crates/rupost-ai::FailureAttributor`）；EngineArbiter/McpToolBridge 停泊中期。AI 红线五条（脱敏先于 AI / plugin_args 不入快照 / masking 感知 / 不可信输入校验 / B1/B2 降级用例强制） | [03 §7] |
| **D10** | 资源约束分级 | MVP 最小集：5s 可配置超时（`--plugin-timeout`）+ 进程组清剿（Unix PGID / Windows Job Object）+ 10MB 载荷上限 + stdin/stdout/stderr 三向排水；`[P2]` RLIMIT 配额 | [03 §8、§10] |
| **D11** | 插件失败分类学 | `timeout / invalid_output / crashed / exit_non_zero` 四类失败与"缺失"共享降级开关；`cache_scope`（`none/request/suite`，`ttl` P2）消除 OAuth2 类插件的进程风暴 | [03 §8.3] |

### crate 分层裁决（补记）

现实 workspace 为双 crate 分层：LLM 客户端/归因/聚类/翻译/合成在 `crates/rupost-ai`（不依赖 rupost 内部类型，B3 编译期可剥离的结构基础）；脱敏原语/确定性规则/编排在 `src/ai`（依赖方向 rupost → rupost-ai）。插件体系的 `PluginRegistry`/`StdioDriver`/`PluginPatch` 归属主 crate `src/plugin/`（06 M2/M3 落点）；AI Miner 类功能若未来内核化，应进 `crates/rupost-ai` 而非 `src/ai`（保持 B0 依赖图：引擎模块对 rupost-ai 零依赖）。

---

## 2. 内核依赖选型决策（保留）

1. **XML 报告生成**：`quick-xml`（serialize 特性），零分配事件流式写入；
2. **终端控制字符剥离**：`strip-ansi-escapes`——XML/快照写盘前净化（终端渲染侧净化统一走同一 SSOT，见 03 §6.4）；
3. **基础哈希加签**：`sha2`、`hmac`（MVP 必引入，现状 Cargo.toml 缺失——01 §1.2 `[待建 M1]`）；`md-5` 降为 `[P2]`（80% 签名场景 HMAC-SHA256/SHA256 已覆盖）。

---

## 3. 风险登记册 (Risk Register)

| ID | 风险 | 防御 | 状态 |
| :-- | :--- | :--- | :--- |
| R1 | JSON 请求体重排破坏签名 | `raw_body: Option<String>` 零拷贝保真（`src/http/request.rs:22` `[已有]`） | 已交付 |
| R2 | 快照静默吞没网络失败现场 | 顶级 `error: Option<ExecutionErrorDetail>` + 故障 100% 录制（02 §2） | 待建 M1 |
| R3 | 项目级插件恶意执行 (RCE) | 命名契约 + `--allow-project-plugins` 闸门 + 项目级 registry 废除 + 路径白名单（03 §3.2） | 规范就绪，M2 落地 |
| R4 | 大包管道死锁与插件时延污染 | 10MB 上限 + 三向异步排水 + EOF 显式关闭；`plugin_duration_us` 与物理网络秒表隔离 | 规范就绪，M3 落地 |
| R5 | 孤儿/僵尸进程泄漏 | Unix PGID 组级 SIGKILL / Windows Job Object；fail-fast 组级 kill 传播 | 规范就绪，M3 落地 |
| R6 | 注册表并发写与幽灵缓存 | 原子重命名 + mtime+4KB 哈希指纹（03 §10.3） | 规范就绪，M2 落地 |
| R7 | 降级假阳性穿透 CI 门禁 | `ExecutionOutcome::DegradedPass` 三态 + `--strict-plugins` 强制 exit 1（03 §10.5） | 规范就绪，M3 落地 |
| R8 | 跨平台路径与 Windows Shebang | `normalize_posix_path` 统一归一（`src/utils/path_guard.rs:12`）+ Windows 解释器透明代理 | 部分已有，M2/M3 补齐 |
| R9 | 发现即执行（供应链 RCE） | **D2 静态 manifest**：发现零子进程（03 §3.0） | 规范就绪，M2 落地 |
| R10 | registry.json 投毒/持久劫持 | **D3** 路径白名单 + 属主校验 + 项目级闸门扩展（03 §3.2） | 规范就绪，M2 落地 |
| R11 | 第三方插件凭据外泄 | **D4** 环境白名单 + `requires_env` 声明制（03 §6.2） | 规范就绪，M4 落地 |
| R12 | 快照默认明文凭据落盘 | 强制 `0600` + `--mask-secrets` 扩面表 + `masking` 标记 + 现场掩码引导（02 §2.7/§4） | 规范就绪，M1 落地 |
| R13 | 快照消费侧不可信输入 | schema 校验 + 64MB 上限 + 报告转义契约（02 §3.3） | 规范就绪，M5 落地 |
| R14 | 终端注入（ANSI/OSC） | manifest 文本白名单净化 + warnings 净化 SSOT（03 §6.4） | 规范就绪，M4 落地 |

---

## 4. 已废除的历史表述（防复活清单）

以下 v2.0.0 表述经对抗性审查裁决废除，任何后续文档不得复活：
1. "插件在安装或探测时响应 `--manifest` 命令"（→ 静态 manifest，D2）；
2. "静默降级为跳过"（→ 三处 WARNING + DegradedPass，D5）；
3. "引擎注入预留 Token / 预置兜底 Header"（→ 用户手写静态 Header 是唯一兜底机制，D5）；
4. "`priority=<int>` 默认 100 / Stage 区间 10~30/40~60/70~90 三套数字体系"（→ 纯 FIFO，D6）；
5. "未知子命令捕获分支"（→ `args[0]` 主动查表，D7）；
6. `status.code().unwrap_or(1)` 退出码穿透（→ 信号感知 `128+signal`，D7）；
7. "四大 AI SPI 内核 trait"（→ AI-as-Plugin，D9）；
8. "`rupost trace analyze snapshot.json`"（→ 幽灵命令，统一为 `replay --explain` [已有] 与 `ai triage --snapshot` [待建]）；
9. "`$random_int` 无参固定区间即为终态"（→ 参数化 `[待建 M1]`）；
10. "约 15MB 二进制"（→ ≤20MB 统一口径，实测基线 18.1 MiB）；
11. "全库 564+ 测试"（→ 基线口径改为"测试基线不回退（现 950+ 属性计数）+ 新增套件全绿"）；
12. "内存常驻 ≤30MB"（→ 改为 CI 观测指标 `/usr/bin/time -l` max RSS，非门槛）。

---

## 5. 执行顺序指针

施工顺序以 [06 §4](./06-mvp-implementation-plan-personas-and-scenarios.md) 的 M1~M5 为唯一权威（本文件 v2.0.0 §5 路线图已删除——其与 06 的双轨冲突是 v2.1.0 重构的直接动因）。AI 能力的切入角度与优先级裁决见 `doc/plans/archive/2026-q4/2026-10-27-ai-master-roadmap.md`（B0~B4 宪法 + 五阶段管线 + E 系列切入点）。
