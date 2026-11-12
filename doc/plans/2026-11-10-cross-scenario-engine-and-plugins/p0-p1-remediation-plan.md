# P0/P1 缺陷解决方案与落地追踪 (Remediation Plan)

- **状态**: Draft (v1.0.0 — 方案已全部裁决并合入 v2.1.0 文档，代码落地待 M1~M5)
- **关联**: [review-adversarial.md](./review-adversarial.md)（审查记录）、[README §0 权威矩阵](./README.md)
- **用途**: 对审查报告 **P0 级发现（多路交叉证实）** 与 **P1 级发现（择要）** 逐项给出解决方案，登记文档落点、代码落点与验收标准，作为 M1~M5 施工期间的**缺陷追踪底账**（每个 P0/P1 必须走到"已落地"才允许回标 Execution Ready）。
- **状态图例**: 📄 方案已合入 v2.1.0 文档 · 🔧 代码待落地（标注 Milestone） · ✅ 已验收

---

## 1. P0 级发现解决方案

### P0-1 加签宏地基为零实现
* **发现**：`$hmac_sha256/$sha256/$md5/$random_string/$random_email` 全库零实现；`sha2/hmac/md-5` 依赖不存在；`$random_int` 实为无参固定区间（`src/variable/resolver.rs:20-31`）。"0 依赖 CI 闭环加签"叙事建立在虚构能力上。
* **解决方案**：
  1. M1 引入纯 Rust 零系统依赖 `sha2`、`hmac`（`md-5` 降 P2）；
  2. 实现 `$hmac_sha256(key, msg)`、`$sha256(msg)`；`$random_int` 参数化为 `(min, max)`；
  3. 已知答案测试锁定：HMAC-SHA256 用 RFC 4231 官方向量，SHA256 用 NIST 向量，`$random_int` 测边界含 min==max；
  4. 体积预算 ≤500KB 增量（M1 前登记 18.1 MiB 基线）。
* **落点**：📄 [01 §1.2](./01-core-capabilities-and-api.md)、[06 M1](./06-mvp-implementation-plan-personas-and-scenarios.md)；🔧 `Cargo.toml` + `src/variable/resolver.rs`（M1）
* **验收**：宏向量全绿；[04 套件 1](./04-testing-strategy-and-scenarios.md) 零子进程 + 宏全流程 + 快照 0600。

### P0-2 快照黑匣子契约：三文档矛盾、payload 空心、无演进策略
* **发现**：`entries`（02）vs `results[0]`（04）；05 承诺顶级 `error` 但 02 Schema 没有；`SnapshotRequest/Response/Assertion/RunnerFingerprint` 从未定义；`schema_version` 注释"恒定 1.0"自封演进死路；快照实际仅 4 字段且不含时序。
* **解决方案**：
  1. **02 收敛为唯一权威**：`entries` 定名；顶级 `error: Option<ExecutionErrorDetail>`（复用 `src/history/model.rs:60` 现有类型，不造平行结构）；4 个 payload 类型完整定义；新增 `masking: MaskingMode` 与 `ExecutionOutcome` 三态；
  2. **演进策略**：字段只增不改、消费者必须容忍未知字段、破坏性变更递增版本、按主版本兼容；
  3. **时序复用语义**：`dns/tcp/tls` 改 `Option<u64>`（None=keep-alive 复用未发生），H2 共享连接语义注明，`duration_us` 与 `total_us` 口径绑定注释；
  4. **迁移映射表**：`SnapshotSuite/SnapshotEntry` → 新 Schema，写入链路 `save_batch_snapshot`（`src/history/storage.rs:304`）随 M1 迁移。
* **落点**：📄 [02 §2 全部](./02-artifact-contract-for-plugins.md)、05 ADR-D8；🔧 `src/history/model.rs` + `storage.rs`（M1）
* **验收**：[04 套件 3 前半](./04-testing-strategy-and-scenarios.md) Schema 契约测试（serde round-trip + 未知字段容忍）。

### P0-3 Stdio IPC 协议残缺（链式折叠不可实现 + 死锁洞 + 失败无分类）
* **发现**：输出缺 `set_variables`、输入缺 hook 身份与前序变量上下文、无版本回显；stdout 日志污染 Patch JSON；stderr 洪泛死锁未防；超时之外四种失败行为全部未定义。
* **解决方案**：
  1. **输入补全**：`hook: {directive, alias, stage}` 身份 + `variables`（内核已解析上下文，承载前序 `set_variables` 链式折叠）+ `env.*` 由内核解析（明文仅过 IPC，`plugin_args` 永不入快照/报告）；
  2. **输出补全**：`protocol_version` 回显 + 失败信封 `{"ok": false, "error_kind", "message"}`；
  3. **管道纪律**：stdout 仅允许单 JSON 对象（多余输出判 `invalid_output`）、日志强制走 stderr、stderr piped 异步排水并 16KB 截断入 warnings、CI 模式不直通终端；
  4. **失败分类学**：`timeout / invalid_output / crashed / exit_non_zero` 四类判定与默认行为表，与"缺失"共享降级开关；`--plugin-timeout` 可配置；
  5. **硬约束**（基线已有，纳入验收）：10MB 载荷上限 + stdin/stdout/stderr 三向独立 Task 排水 + `shutdown().await` EOF + 进程组清剿。
* **落点**：📄 [03 §8 全部 + §10.1](./03-plugin-spi-and-degradation.md)、05 ADR-D10/D11；🔧 `src/plugin/driver.rs` + `pipeline.rs`（M3）
* **验收**：[04 §2.5 六件套 mock 矩阵](./04-testing-strategy-and-scenarios.md)（normal / >64KB / >5s / 非法 JSON / exit 3 / stderr 洪泛）+ 并发×插件组合。

### P0-4 CLI 双模路由前提在 clap 现实下不成立
* **发现**：根命令 `trailing_var_arg = true`（`src/cli/mod.rs:29`）使 `rupost flamegraph` 被吸入 `cli.args`，"未知子命令捕获"分支根本不存在；内置/插件/HTTP 方法三方命名空间无裁决序；`unwrap_or(1)` 使信号退出码穿透失真。
* **解决方案**：
  1. 不依赖 clap 未知命令分支，**主动拦截 `args[0]` 查表**：内置子命令 > 插件命令（精确匹配）> HTTP 方法名（大小写不敏感）> 报错 exit 2；
  2. 重名冲突：内置/方法名赢 + 安装期告警（03 §3.1）；
  3. 退出码信号感知：`Some(code)` 原值穿透（三值契约仅约束内置命令），`None → 128+signal`。
* **落点**：📄 [03 §6.1/§6.2](./03-plugin-spi-and-degradation.md)、05 ADR-D7；🔧 `src/cli/dispatch.rs` + `plugin_cmd.rs`（M4）
* **验收**：M4 Red 用例——真实子进程 `exit 7` 原值穿透、信号杀死 `128+signal`、registry 命中但二进制已删 → exit 1 + 明确错误消息；内联模式位置契约回归全绿（`test_global_args_position_contract_table` 不回退）。

### P0-5 安全模型缺"默认不信任"地基（五项）
| # | 发现 | 解决方案 | 落点（📄 文档 / 🔧 代码） | 验收 |
| :-- | :--- | :--- | :--- | :--- |
| 5.1 | **发现即执行**：`--manifest` 探测使扫描变成执行陌生二进制（npm postinstall → RCE） | 静态 manifest 文件 `rupost-plugin-<name>.json` 与二进制同目录；发现 = readdir + JSON 解析**零子进程**；`--manifest` 降级为显式 `plugin sync --regenerate` [P2] | 📄 03 §3.0 / 🔧 M2 `src/plugin/registry.rs` | 守卫测试 `test_discovery_spawns_no_processes`；反转"发现即 spawn"必红 |
| 5.2 | **registry.json 投毒**：篡改缓存即持久劫持；项目级 registry 绕闸；路径穿越；mtime 单指纹 TOCTOU | 可执行路径 canonicalize 白名单（拒 `..`/逃逸 symlink）；`~/.rupost` 0700 / registry.json 0600 + 属主 uid==euid 校验；**项目级 `./.rupost/registry.json` 文件废除**（项目插件只走 plugins 目录 + 闸门）；mtime + 前 4KB 哈希双指纹 + `tmp.<pid>.<seq>` 原子重命名 | 📄 03 §3.2/§10.3 / 🔧 M2 | 路径穿越/属主/幽灵缓存三类负向用例全拒 |
| 5.3 | **全量 env 继承**：第三方插件拿走父进程全部凭据 | 白名单 `PATH/HOME/LANG/TERM/TMPDIR + RUPOST_*`；manifest `requires_env` 声明 + `--plugin-env <VAR>` 显式放行 | 📄 03 §6.2 / 🔧 M4 | M4 env 白名单断言用例（子进程 env 精确集合） |
| 5.4 | **快照默认明文凭据落盘**：/tmp 0644 + 加签后"消息+HMAC 对"同入库 = 爆破验证 oracle | 落盘强制 `0600`（与 umask 无关）；`--mask-secrets` 扩面表（Authorization/Cookie/Set-Cookie/X-Sign/query token/body 密码键，表驱动单测）；掩码在**变量解析之后**；`masking` 标记供消费侧判断敏感性；现场文档引导开启掩码 | 📄 02 §2.7/§4 / 🔧 M1（权限+掩码扩面） | 权限断言用例 + 掩码表驱动单测全绿 |
| 5.5 | **降级后未签名请求静默发生产** | 统一降级语义：插件不注入 + 请求按 `.http` 原样外发 + 终端/报告/快照三处 WARNING + `DegradedPass` + 降级计数汇总；**含未解析 `{{var}}` 的 Header 视为兜底缺失 → 跳过该请求**；CI 侧 `--strict-plugins` 硬门禁（降级即 exit 1）兜底；残余风险记录于 05 ADR-D5 | 📄 03 §2/§10.5 / 🔧 M3 | 套件 2 三态用例 + 未解析变量守卫用例 + 降级三处 WARNING 断言 |

---

## 2. P1 级发现解决方案

### P1-1 降级语义三重拼写 + 三种兜底机制
* **解决**：`?optional` 后缀为**唯一**声明语法（`optional=true`、`@use optional: true` 废除）；兜底鉴权唯一机制 = 用户手写静态 Header；禁"静默"（反静默哲学）与"引擎注入预留 Token"（05 防复活清单第 2/3 项）。
* **落点**：📄 03 §2.1/§2.2、05 §4；04 套件 2 已同步 ✅（文档层闭环）
* **验收**：套件 2 全绿即落地验收（M3）。

### P1-2 priority 三套数字体系互斥
* **解决**：彻底废除 priority 魔数，同阶段**纯 FIFO 书写行序**（所见即所发）；manifest 删除 `default_priority` 字段；不存在跨阶段 priority；`@pipeline` 标 P2 且与阶段序冲突 = 语法错误 exit 2。
* **落点**：📄 03 §4.1/§5.2、05 ADR-D6；🔧 M1（AST）+ M3（管线排序）
* **验收**：套件 5 执行序列断言 `[a1, t2, t1, s1]`。

### P1-3 MVP 权威缺失（@pipeline/@use 三种说法）
* **解决**：06 §3 为范围唯一权威；`@use` 缩进块与 `@pipeline` 标 `[P2 预览]`；05 降级为 ADR 索引并删除互斥路线图；README §0 权威矩阵固化。
* **落点**：📄 06 §3、05、03 §4.1 ✅（文档层闭环）
* **验收**：文档治理闭环（已完成）；代码层无动作。

### P1-4 注册表新鲜度无闭环（新装插件假降级）
* **解决**：运行时对插件根做**目录级 stat**（readdir + mtime/size，零子进程，≤5ms 预算）；manifest 集合/指纹变化 → 条目失效并从静态 manifest 重建；新装插件在 sync 前即可被发现；alias 冲突：项目级覆盖全局 + 启动 WARNING。
* **落点**：📄 03 §3.0/§3.1；🔧 M2
* **验收**：`test_registry_cache_staleness` 扩展"新装插件即时发现"用例；"删除插件后陈旧条目不执行"用例。

### P1-5 退出码契约冲突（穿透 vs 三值门禁）+ AST exit 2 虚构
* **解决**：三值契约（0/1/2）**仅约束内置命令**；插件子命令原值穿透、信号 `128+signal`；AST 语法错误显式归 2（现状 exit 1 → M1 Red 用例 + dispatch 归位改造 + 守护测试）；降级相关退出归 1。
* **落点**：📄 01 §2.1、03 §6.2；🔧 M1（exit 2 归位）+ M4（穿透）
* **验收**：`tests/run_exit_code_test.rs` 扩展 AST→2 用例；M4 穿透用例（见 P0-4）。

### P1-6 @auth 无缓存语义（100 请求 = 100 次取 Token 子进程）
* **解决**：manifest 新增 `cache_scope: none|request|suite`（`ttl` P2）；内核按 scope 复用 Patch 结果；OAuth2 类插件声明 suite 级缓存。
* **落点**：📄 03 §3.1/§8.3、05 ADR-D11；🔧 M3
* **验收**：并发×插件组合测试 + DoD 量化指标"100 请求 × 1 @auth 套件子进程总数 ≤ 2"。

### P1-7 04 防护网不可编译/不可落地
* **解决**：语法对齐（`@sign non_existent_signer ?optional`）；`results`→`entries`、`crate::`→`rupost::`；`{{$body}}` 废除（请求体引用不属宏域，求值顺序由 src/variable 守护测试锁定）；套件 1 语义重定义（零子进程派生断言 + `env -i`/PATH 空目录 + CI 容器 job + musl target 登记）；套件 4 改 tokio `TcpListener` 原始字节监听（WireMock 废除）；新增六件套负向矩阵与正/反对照组（反假守护）；§4 套件 owner 映射到 M1/M3/M5。
* **落点**：📄 04 全文 v2.1.0 ✅；🔧 随 M1/M3/M5 落地
* **验收**：各套件全绿；反转生产逻辑必红（正向对照组已内置）。

### P1-8 脱敏承诺互斥（保真验签 vs 掩码）
* **解决**：`masking: none|secrets` 标记为调和机制——默认保真支撑字节级验签；掩码模式下验签/Diff 类插件**MUST 附带"数据不完整"警告**（消费契约写入 02 §2）；掩码覆盖面表驱动 + 表驱动单测。
* **落点**：📄 02 §2/§4 ✅；🔧 M1（掩码扩面 + 权限）
* **验收**：掩码表驱动单测 + masking 消费契约测试。

### P1-9 crates/rupost-ai workspace 被文档无视
* **解决**：crate 分层裁决写入 05——LLM 客户端/归因/聚类/翻译/合成在 `crates/rupost-ai`（不依赖 rupost 内部类型，B3 可剥离结构）；脱敏原语/规则/编排在 `src/ai`；插件体系 `PluginRegistry/StdioDriver/PluginPatch` 归主 crate `src/plugin/`；未来 AI Miner 内核化必须进 `rupost-ai`（维持 B0：引擎模块对 rupost-ai 零依赖）。
* **落点**：📄 05 crate 分层裁决段 ✅；🔧 M2/M3 模块落点遵循
* **验收**：`grep "use rupost_ai"` 仅命中 `src/ai` 与命令族；`src/runner` 无 rupost-ai 依赖（CI grep 守护）。

### P1-10 M4 退出码 127 测试设计错误 + 接口冻结缺失
* **解决**：
  1. 测试修正：删除"127 或 1"断言（127 为 shell 语义，直接 exec 拿不到）；改为真实子进程 `exit 7` 原值穿透 + 信号 `128+signal` + 缓存过期二进制已删 → exit 1；
  2. 接口冻结门禁：M1 出口 = 《接口冻结清单》（`PluginDirective` 参数表示有序对、`PluginStage` 三值枚举、`ExecutionSnapshotEntry` 字段集与 `_us` 单位制、`ExecutionOutcome`）；M2 出口 = manifest 字段集与 IPC/路由键映射；清单变更须独立评审。
* **落点**：📄 06 M4/M1/M2 ✅；🔧 M1 起执行
* **验收**：冻结清单文件随 M1/M2 提交入库；后续 PR 触碰冻结面时评审记录可查。

---

## 3. 追踪汇总表

| 编号 | 状态 | 文档 | 代码 Milestone |
| :-- | :--- | :--- | :--- |
| P0-1 | ✅ 已落地（M1-A：sha2/hmac + RFC 4231 向量） | 01 §1.2 / 06 M1 | ✅ M1 |
| P0-2 | ✅ 已落地（M1-B：Schema v1.0 + 迁移 + 契约测试 + replay 双格式兼容） | 02 §2 / 05 D8 | ✅ M1 |
| P0-3 | ✅ 已落地（M3：StdioDriver 失败分类学 timeout/invalid_output/crashed/exit_non_zero + 三向排水 + 进程组熔断 + 6 件套 mock 矩阵） | 03 §8 / 05 D10-D11 | ✅ M3 |
| P0-4 | ✅ 已落地（M4：args[0] 裁决序内置>插件>方法>URL>未知 exit2 + 信号感知穿透 128+signal） | 03 §6 / 05 D7 | ✅ M4 |
| P0-5.1 | ✅ 已落地（M2：静态 manifest 零子进程发现 + 发现零派生守护测试） | 03 §3.0 / 05 D2 | ✅ M2 |
| P0-5.2 | ✅ 已落地（M2：属主/0600 信任校验 + 路径穿越与 symlink 逃逸拒绝 + 空缓存假命中回归修复） | 03 §3.2 / 05 D3 | ✅ M2 |
| P0-5.3 | ✅ 已落地（M3/M4：spawn 环境白名单 + requires_env + --plugin-env + env 泄漏测试） | 03 §6.2 / 05 D4 | ✅ M3/M4 |
| P0-5.4 | ✅ 已落地（0600 + masking 标记 + redactor SSOT + 掩码 smoke 测试） | 02 §2.7/§4 | ✅ M1/M3 |
| P0-5.5 | ✅ 已落地（M3：统一降级三件套 + 未解析变量守卫 + DegradedPass + --strict-plugins 门禁 6 用例 e2e） | 03 §2 / 05 D5 | ✅ M3 |
| P1-1 | ✅ 已落地（`?optional` 唯一拼写 + `optional=` 显式报错，parser + e2e 双锁定） | 03 §2 / 05 §4 | ✅ M1 |
| P1-2 | ✅ 已落地（M1：FIFO 仲裁 + 魔数废除显式报错 + 引号分词） | 03 §4/§5 / 05 D6 | ✅ M1 |
| P1-3 | ✅ 已闭环 | 06 §3 / 05 / README §0 | — |
| P1-4 | ✅ 已落地（M2：目录级 stat 清单集合完全相等判定 + 空缓存假命中回归 + bench 0.58ms ≤5ms） | 03 §3.0 | ✅ M2 |
| P1-5 | ✅ 已落地（M4：插件原值穿透 e2e exit7 + 未知命令 exit2） | 01 §2.1 / 03 §6.2 | ✅ M4 |
| P1-6 | ✅ 已落地（M3：cache_scope none/request/suite + Hook 缓存复用） | 03 §3.1/§8.3 / 05 D11 | ✅ M3 |
| P1-7 | ✅ 已落地（M1-M5：04 套件 1/2/3/4/5 全部落地，owner 映射兑现） | 04 全文 | ✅ M1-M5 |
| P1-8 | ✅ 已落地（masking 感知红线：ai triage-snapshot 读取 masking 标记并输出数据不完整声明） | 02 §2/§4 | ✅ M5 |
| P1-9 | 📄 已裁决（分层边界由代码结构遵守：`use rupost_ai` 仅存在于 src/ai 与 ai 命令族；CI grep 守护登记待办） | 05 分层裁决 | ✅ M2/M3 |
| P1-10 | ✅ 已落地（M1 冻结清单 interface-freeze-m1.md + exit 127 测试废除改原值穿透 e2e） | 06 M1/M2/M4 | ✅ M1/M4 |

> 汇总裁决：**全部 P0/P1 均已有明确解决方案并合入 v2.1.0 文档层；剩余工作全部是带 Milestone 的代码落地项**。本底账随实施推进更新状态列（🔧 → ✅），M5 结束时必须全表 ✅ 方可回标 Execution Ready。

## 4. M6 工作项（v2.1.1 新增：插件运行时就绪 + 缺口收尾，03 §3.1.1/§8.3）

| ID | 工作项 | 归属 |
| :-- | :--- | :--- |
| M6-1 | `runtime` manifest 声明解析 + readdir 预检 + 插件三态状态机（ready/pending/failed）+ `plugin list` 可视化与处方提示 | M6 |
| M6-2 | 失败分类学扩展 `runtime_missing`/`dependency_missing`（stderr 表驱动特征，提取缺失模块名入处方） | M6 |
| M6-3 | `requires_env` 声明未放行启动 WARNING | M6 |
| M6-4 | ✅ 已落地（2026-11-12：`--max-snapshot-body-bytes` 旗标接线，`save_batch_snapshot` 参数化上限，截断 + SHA-256 验真守护测试 + verify_features 冒烟） | M6 |
| M6-5 | ✅ 已落地（2026-11-12：main 入口 `RupostError::ParseError` → exit 2 归位，二进制级守护 `tests/ast_exit_code_test.rs` 含 exit 1 防假守护对照） | M6 |
| M6-6 | DoD 性能验证测试：时延隔离不变量 / suite cache_scope 进程数 ≤2 / fail-fast 组级 kill 传播 / 并发×插件组合 | M6 |
| M6-7 | 04 套件 3 录制保真半落地 `tests/artifact_mining_test.rs` | M6 |
| M6-8 | 掩码扩面表驱动单测（X-Sign/query token/Set-Cookie/body PII 全表） | M6 |
| M6-9 | P1-9 CI grep 守护（`use rupost_ai` 边界） | M6 |

> M6 完成后底账维持全 ✅ + M6 全 ✅，方可维持 Execution Ready。

> 差距收口批注（2026-11-12，plugin-m6 工作区）：代码-设计偏差三项同批落地——①`cache_scope=request` 请求内复用实现（此前枚举无分支退化为 none，`PluginPipeline` 请求局部备忘录 + TDD 防假守护对照 suite 语义）；②registry `register_from_manifest` 保留字阶段（post_response/on_failure/harvest）拒绝注册（interface-freeze-m1 §1 裁决落地，此前静默注册死钩子）；③manifest `ttl` P2 保留值告警 + 按 none 归位（此前 serde 静默忽略与 types.rs 注释裁决不符）。`verify_features.sh` 补插件体系冒烟步骤 15（Stdio IPC 注入防假守护 + 工作区闸门 + 降级 DegradedPass 三态 + 保留字告警 + 新旗标 + exit 2）。

> 全量落地记录（2026-11-11，M2~M5 收官）：M2 registry/缓存硬化（bench registry_cache_read 0.58ms ≤5ms）→ M3 StdioDriver/失败分类学/统一降级/--strict-plugins（6-driver mock 矩阵 + 降级 6 用例 e2e 含正向对照组）→ M4 args[0] 裁决序 + TOFU/环境白名单/信号感知穿透（e2e exit7 原值穿透）→ M5 plugin list/schema + ai triage-snapshot（masking 感知）+ standalone/byte-fidelity 套件。三层门禁：workspace 1029 测试 0 失败 / verify_features.sh PASS / examples run_all 15 套 ALL PASS；clippy -D warnings + fmt 零漂移。release 二进制净增量 vs 审查基线 +141 KiB（预算 ≤500 KiB）。**P0/P1 底账全表 ✅，Execution Ready 回标条件达成。**
