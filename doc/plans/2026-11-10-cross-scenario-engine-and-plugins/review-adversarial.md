# 对抗性审查记录 (Adversarial Review Record)

- **审查日期**: 2026-11-10 ~ 2026-11-11
- **审查方式**: 4 路独立子代理对抗审查 + 交叉验证汇总：
  1. **文档-代码库事实核查官**（逐条断言 → file:line 证据 → 裁决）；
  2. **架构设计与协议一致性对抗官**（文档间矛盾/协议不完备/语义漏洞）；
  3. **安全对抗官**（STRIDE + 供应链威胁建模）；
  4. **范围裁决与可测性对抗官**（YAGNI / 测试可落地性 / 里程碑工程性 / 验收指标证伪）。
- **审查对象**: 本目录 v2.0.0 基线 7 份文档；对照 rupost 代码库现状（commit `0f2c9097` 前后）。
- **产物**: 本记录 + 全部 7 份文档的 v2.1.0 修订。**v2.1.0 全部为 Draft 待评审状态**（落实总体裁决"降回 Draft"）；人工评审通过后方可回标 Approved / Execution Ready 并启动 M1 施工。

---

## 1. 总体裁决

**选型方向正确，v2.0.0 文档不可执行（Not Execution Ready）**。方案 D（Stdio IPC）+ 统一注册表 + 快照黑匣子 + 降级哲学的架构意图成立；但文档存在统一失真模式——**把"待建目标"用现在时写成"既有能力"**，且 7 份文档在三个"协议层"（快照 Schema、IPC 协议、路由/优先级裁决）上各自为政。按 v2.0.0 开工，M1~M5 会在实现期被迫就地发明规范。

## 2. 核心发现（按裁决归组）

### A. 现状虚构（事实核查官证实）
| 断言（v2.0.0） | 事实 |
| :--- | :--- |
| `$hmac_sha256/$sha256/$md5/$random_string/$random_email` 开箱即用 | 全库零实现；`sha2/hmac/md-5` 依赖不存在；实际仅 5 宏（`src/variable/resolver.rs:20-31`），`$random_int` 无参固定区间 |
| 快照含时序/告警/Socket 诊断（现在时） | 实际 `SnapshotSuite/SnapshotEntry` 仅 4 字段；时序只在 diagnose/web 路径且全毫秒 |
| `rupost plugin list/sync/schema`、`template aws-sigv4` | 不存在；`template` 是 `init` 别名（`src/cli/commands/mod.rs:58`） |
| exit 2 = AST 错误 | 现状语法错误 exit 1；2 仅为 clap 参数错误隐式默认 |
| 正则断言、`--allow-missing-plugins` | 均零实现 |
| 二进制约 15MB | 实测 18.1 MiB |
| `ai triage --report-file`、`trace analyze` | 前者为位置参数且消费报告 JSON 非快照；后者为幽灵命令 |

### B. 设计与协议矛盾（协议官证实）
1. 快照契约三方矛盾：`entries`（02）vs `results`（04）；05 承诺顶级 `error` 但 02 无此字段；`SnapshotRequest` 等 4 类型从未定义；`schema_version` 注释"恒定 1.0"自封演进死路；
2. IPC 协议：输出缺 `set_variables`、输入缺 hook 身份/前序变量上下文、无版本回显、stdout 污染/stderr 死锁未封口、失败分类学缺失；
3. priority 三套数字体系（默认 100 / Stage 区间 / manifest 20）互斥；
4. `@pipeline`/`@use` 在 03（Approved 可用态）与 06（Out of Scope）与 05（Phase 0）三种说法；
5. `trailing_var_arg` 使"未知子命令捕获"分支不存在，路由前提失效；三方命名空间无裁决序；
6. 降级语义："静默"vs"明确警告"自相矛盾；"预置兜底 Header/预留 Token"三种机制无来源；
7. 04 测试代码引用不存在语法/路径（`@plugin`、`{{$body}}`、`crate::` 于集成测试）；退出码 127 测试设计错误（直接 exec 拿不到）；
8. `@auth` 无缓存语义（100 请求 = 100 次取 Token 子进程）；注册表新鲜度无闭环（新装插件必遭假降级）。

### C. 安全威胁（安全官证实，STRIDE + 供应链）
1. **P0 发现即执行**：`--manifest` 探测协议使扫描变成执行陌生二进制（npm postinstall → RCE）；
2. **P0 registry.json 投毒**：持久劫持任意子命令；项目级 registry 绕过 `--allow-project-plugins` 闸门；路径穿越未校验；mtime 单指纹 TOCTOU；
3. **P0 全量 env 继承**：第三方插件拿走父进程全部凭据；
4. **P0 快照默认明文凭据落盘**：/tmp 0644 + 加签后"消息+HMAC 对"同入库 = 离线爆破验证 oracle；
5. P1：降级后未签名请求静默发生产；manifest 文本 ANSI 注入；`plugin_args` 密钥流落盘未定义；TTY 直通内核失明 + UI 伪造。

### D. 范围与可测性（可测性官证实）
1. 快照契约在 06 五里程碑零 owner；04 四套件中三个零 owner（"防护网"与"施工计划"脱节）；
2. YAGNI：四大 AI SPI trait 无消费者；`plugin sync` 无在线源；`$md5/$random_email` 无需求锚点；
3. 验收指标不可证伪：≤100KB（sha2+hmac 预估 100~300KB）、TTFB 误差为 0（无基准量）、内存 ≤30MB（无测量）、564+（实测 950+，陈旧）；
4. 测试落地性：PATH 清空在 macOS/CI 证明不了"无解释器"；WireMock（Java）违反零依赖且拿不到原始字节；`run_isolated_test` 为虚构 helper；
5. 接口冻结缺失：M1 的 AST 形状决定 M3 的 IPC、M2 的 manifest 决定 M4 路由键，无冻结门禁；
6. `crates/rupost-ai` workspace 被 7 份文档整体无视（现实双 crate 分层）。

## 3. 修订索引（v2.0.0 → v2.1.0）

**注**：审查进行期间，文档在协作 workspace 中已演进一版（"Review Defense Specs"），先行吸收了部分发现（IPC 输出补 `set_variables`/hook 身份、stderr 排水 + 10MB 上限、进程组清剿、复合键索引、原子重命名 + 4KB 哈希指纹、Windows shebang、三态判定 + `--strict-plugins`、快照 256KB 截断 + sha256、priority 魔数废除改纯 FIFO、`@use` 标 Phase 2、README 计数）。v2.1.0 修订在该基线上叠加以下裁决：

| 文档 | 修订内容 |
| :--- | :--- |
| **02** | 快照唯一权威声明；现状基线与迁移映射表；补全 `SnapshotRequest/Response/Assertion/RunnerFingerprint` 定义；`masking: MaskingMode` 字段；`TimingWaterfallUs` 连接复用 `Option` 语义 + H2 说明；`schema_version` 演进策略；安全不变量（`plugin_args` 永不入快照、`0600`）；消费前校验；命令拼写裁决（`trace analyze` 废除、`ai triage --snapshot` 待建）；掩码扩面表 + `[REDACTED]` 占位符 |
| **03** | 发现机制安全裁决（静态 manifest 零子进程，废除 `--manifest` 执行式探测）；缓存安全硬化（0700/0600、路径白名单、项目级 registry 废除、PATH 信任降级）；降级语义统一（不静默、不注入魔法值、未解析变量守卫、缺失/失败共享开关、与三态门禁衔接）；`@pipeline`/`@use`/IntelliSense 标 P2；路由裁决序（`args[0]` 查表 + 内置>插件>方法名）；环境白名单 + TOFU 横幅 + 信号感知退出码；manifest 文本净化；**AI-as-Plugin 裁决取代四大内核 SPI**；IPC 补 `variables` 上下文/版本回显/失败信封/stdout 单对象纪律/失败分类学四表/`cache_scope`/`--plugin-timeout` |
| **01** | 全文 `[已有]`/`[待建 M<N>]` 基线标注；宏现状表 + 待建清单；体积口径 ≤20MB（实测 18.1 MiB）；时序精度口径声明 + 证书链 best-effort；正则断言标 P2；exit 2 标待建；`--only` 依赖闭包/teardown 语义；`plugin sync` 裁撤；`template` 别名冲突登记；`--strict-plugins` 退出码语义保留 |
| **04** | 套件 1 语义重定义（零子进程 + env -i + 容器 job + musl 登记）；套件 2 原样外发断言 + 正向对照组反假守护 + 未解析变量守卫；套件 3 拆 Schema 契约/录制保真双测试 + `rupost::` 路径 + 复用语义断言；套件 4 WireMock → tokio TcpListener；新增 2.5 负向矩阵（六件套 mock 插件、并发×插件、快照容错、fail-fast 组级清剿）；§4 套件 owner 映射 |
| **05** | 降级为 ADR 索引：D1~D11 决策记录 + crate 分层裁决 + 风险登记册 R1~R14 + **已废除历史表述防复活清单（12 项）**；删除与 01/03/06 复述及互斥路线图 |
| **06** | Scope SSOT 声明；§3 纳入项 8~10（内置宏/快照契约/AI-as-Plugin）；M1 增补宏 + 快照 Schema + 接口冻结出口条件 + 体积基线登记；M2 静态 manifest + 安全用例 + bench + 冻结清单；M3 失败分类学 + 六件套 mock 矩阵 + 组级清剿 + bench；M4 裁决序 + 退出码测试修正 + 环境白名单 + TOFU；M5 `ai triage --snapshot` 消费者；场景 C"自动注入预留 Header"修正；DoD 全部改为可证伪口径 |
| **README** | v2.1.0 状态 + §0 治理与权威矩阵 + 基线标注约定 + 审查记录链接 + 降级矩阵行修正 |

## 4. AI 功能切入角度裁决（补录）

既有规划（`doc/plans/archive/2026-q4/2026-10-27-ai-master-roadmap.md` B0~B4 宪法 + E 系列切入点）已裁决主战场为"失败处理"；本任务新增的最大角度是**离线快照黑匣子挖掘**。落地裁决：
1. 快照契约为一切黑匣子 AI 的 P0 前置（06 M1 owner 已补）；
2. AI 扩展机制复用插件方案 D（AI-as-Plugin），内核不新增 AI trait（05 ADR-D9）；
3. 首个内置消费者 `ai triage --snapshot`（M5），复用 `crates/rupost-ai` 归因能力；
4. AI 红线五条（03 §7.2）；EngineArbiter/McpToolBridge 停泊中期；
5. 落地顺序：快照契约收口 → E2 就地归因 → E5 语义聚类升级 → 快照黑匣子挖掘 → MCP 中期。

## 5. 遗留待办（本次修订未覆盖，需后续处理）

1. `doc/plans/archive/2026-q4/2026-10-28-ai-feature-roadmap.md` 等既有 AI 文档与 AI-as-Plugin 裁决的对齐（由 AI 主线独立处理）；
2. musl 静态构建 target 与 CI 容器化裸机 job 的具体流水线设计（06 已登记任务）；
3. `progress_summary.md` 与 `checkpoint.md` 的进度归档（随 M1 开工执行，AGENTS.md Stage 6）；
4. v2.0.0 → v2.1.0 的 CHANGELOG 条目（随首次实施提交一并登记）。
