# 脱敏判定可扩展性与掩码可读性 PRD（export/generate 凭据分工增强）

> 由 `make prd NAME=<名>` 生成；产物落在 `doc/plans/<日期>-<名>.md`。

- **状态**：Accepted（已评审通过，6 项架构修订已吸收）
- **日期**：`2026-11-10`
- **执行队列**：独立（并入执行队列，准备实施）
- **关联文档**：
  - [`2026-11-10-redaction-extensibility-implementation-plan.md`](./2026-11-10-redaction-extensibility-implementation-plan.md)（技术实施方案与盲区攻坚）
  - `doc/plans/2026-10-28-requirements-matrix.md`（X 系列同根：safety 画像与环境感知判定）
  - `doc/plans/2026-10-27-generation-semantics-execution-plan.md`（T1 dynamic / T2 trace 生成语义）
  - `src/exporter/redact.rs` / `src/generator/http.rs`（现状实现锚点）
- **事实源分工**：本文件只承载需求与规格，**不记进度**——进度唯一事实源是
  `doc/plans/progress_summary.md`，完成后在其中登记批次行。

---

## 一、背景与痛点

<!-- 每个痛点必须实证（文件:行 / 会话记录 / 复现步骤），不允许"我觉得"。 -->

### P1 敏感键词表硬编码，不可扩展、难维护

`src/exporter/redact.rs:4-24` 的 `is_sensitive_key` 将词表内联于 `matches!` 控制流：显式键与子串族
（`secret/token/password/...`）全部写死在函数体内。后果：

- 新增词表外真实凭据键（如 `x-sentry-key`、`fastly-key`、`session-id`）必须修改源码并重新编译；
- 用户无法按项目定制，长尾键只能漏报；
- 该函数是全库判定 SSOT，消费面 5 处——exporter 导出脱敏、`ai/redactor.rs:25,55`（报告与
  LLM 上下文）、`generator/http.rs:156`（凭据模板化）、`generator/trace.rs:181`（低熵过滤）、
  history 快照掩码——一处受益即处处受益，但扩展只能走代码路径。

### P2 子串误报导致过度脱敏，可读性反向受损

子串 `"token"` 命中 `next_token` / `page_token` 分页游标：`export --redact` 后游标被掩，
复制出去的命令丢失翻页参数，断言调试断裂。**过度脱敏与漏脱敏同样破坏可读性**，现状无豁免机制。

### P3 `[REDACTED]` 单一形态信息量为零

占位符不区分键名、认证 scheme、长度、数量；人读导出产物时无法核对"藏了什么"。与 generate 侧
不对称——generate 已有自描述清单（`generator/http.rs:67` 的 `# Variables: ...` 文件头），
export 没有对应的脱敏清单。

### 设计约束（先立规矩，一切方案在此边界内）

1. **三出口分工是已裁决事实**（T10 审计 + `generator/http.rs:20` 注释原文）：export 给人看 /
   generate 给机器跑 / TUI 还原保真值——本 PRD 不得改变分工；
2. **"一眼死"可供性**：占位符必须一眼可知不可复制执行；
3. **不撞型**：export 占位符形态不得与 `{{}}` 模板语法视觉混淆；
4. **零真值泄露底线**；键名本就明文于命令行，占位符携带键名/长度属零额外泄露。

---

## 二、用户故事（Stage 0 合规）

<!-- Given / When / Then 形式；每条故事对应可验证的验收标准。 -->

**US1 项目级扩展（不改代码加新隐私键）**
Given 团队 API 使用词表外键 `X-Sentry-Key`，
When 在 `rupost.toml [export.redaction] extra_keys` 添加后运行 `rupost export curl --redact`，
Then 该 header 值输出为 `[REDACTED]`，footer 清单列出 `X-Sentry-Key`；`generate` 将其模板化为
`{{x_sentry_key}}`。

**US2 误报治理**
Given 用例含 `next_token=abc` 分页游标（默认被子串 `token` 命中），
When 经 `ignore_keys` 或 `# @no-redact next_token` 声明后 `export --redact`，
Then `next_token` 原样出现在导出命令中，footer 清单不含它。

**US3 团队共享与个人例外**
Given 仓库根 `rupost.toml` 进 git、个人另有 `~/.config/rupost/rupost.toml`，
When 团队成员运行 export，
Then 项目级 `extra_keys` 生效；个人全局配置沿用既有级联查找语义（仅当项目级未定义时可达）。

**US4 文件级标注**
Given `.http` 内 `# @sensitive X-Internal-Key` 与 `# @no-redact next_token`，
When 运行 export / generate，
Then 标注优先于内置启发式；标注随活文档进 git，团队拉取即共享判定。

**US5 上游贡献**
Given 某键成为跨项目通用事实，
When PR 修改 `EXACT_KEYS` 常量并增加一行决策表测试，
Then 所有用户开箱生效；贡献者改动 = 数组一行 + 测试一行。

**US6 可读自验**
Given 任意 `export --redact` 产物，
When 人读 footer 清单，
Then 无需解掩码即可知道藏了哪些键、各认证 scheme、长度与数量。

---

## 三、需求清单

<!-- R1/R2... 编号；每条注明：优先级、验收标准、是否确定性兜底项。 -->

### 一期（本 PRD 实施范围）

| # | 需求 | 优先级 | 验收标准 | 确定性兜底 |
|---|---|---|---|---|
| R1 | 表驱动词表提取：`EXACT_KEYS`/`SUBSTRINGS` 常量化，`is_sensitive_key` 改为查表 | P0 | 决策表单测锁定表内容；全量回归零语义变化（既有 500+ 测试即守护） | 是 |
| R2 | `RedactionPolicy` + `[export.redaction]` extra_keys/ignore_keys 装配 | P0 | 经既有 toml 级联查找（cwd→父目录→`~/.config`）；5 消费点以默认参数接入（不传即 `builtin()`）零破坏；优先级决策表覆盖 extra 命中 / ignore 豁免 / 层间优先级 | 是 |
| R3 | 硬键不可豁免护栏 | P0 | `ignore_keys`/`@no-redact` 仅豁免 SUBSTRINGS 层命中；EXACT 命中（`authorization`/`cookie` 等）永远敏感；守护测试锁定 | 是（安全底线） |
| R4 | 脱敏清单 footer | P0 | export 尾部（stderr）输出 `Redacted N credential(s)` + 逐键 键名/scheme/长度；stdout 命令行保持 `[REDACTED]`；`--report json` stdout 纯净契约不受影响 | 是 |
| R5 | 文件内标注 `@sensitive` / `@no-redact` | P1 | metadata 解析器新增指令（现状未识别静默忽略 ⇒ 旧文件零回归）；优先级 标注 > 配置 > 内置；`@no-redact` 同受 R3 约束 | 是 |
| R6 | 内置误报治理基线 | P1 | `next_token`/`page_token`/`cursor` 进入内置 ignore 基线（测试行锁定）；普通 token 键不受影响 | 是 |

### 二期（登记不实施，独立评审后再立项）

| # | 需求 | 说明 |
|---|---|---|
| R7 | `Sensitivity` 枚举分级（`None/Auth/Credential/Pii` + `classify_key`） | 为 R8 供类型信息；签名迁移涉及 5 消费点全链回归 |
| R8 | `--redact safe\|typed\|shape` 档位 | typed: `[REDACTED:api-key]`；shape: 长度保持 `••••(+N)`（须文档化前缀泄露与同值关联两项风险） |
| R9 | 值形态识别（JWT/hex-32/base64 形状正则） | 仅 shape 档辅助信号，不进默认路径；generate 侧禁用（防误判业务值模板化） |

**判定优先级（终态语义）**：`@sensitive` > 配置 `extra_keys` > 内置 EXACT > 内置 SUBSTRING（可被
ignore 豁免）> 不敏感；`@no-redact`/`ignore_keys` 只作用于 SUBSTRING 层。

---

## 四、非目标

<!-- 显式裁剪，防止范围蔓延；每条一句话理由。 -->

- 不改变 export 默认档位与 `[REDACTED]` 占位符形态——"一眼死"可供性是特性，safe 即现状；
- 不做 `<token>` 类"可填"占位形态——与 `{{}}` 机器语法撞型，破坏两出口分工（约束 3）；
- 不让 `@no-redact`/`ignore_keys` 影响 generate 硬键模板化防线——`authorization` 明文落盘零容忍；
- 不引入熵计算/密码学检测库——R9 用固定形状正则，二期另议；
- 不动 TUI 还原保真值与 history 本机明文两项既有裁决（T10 事实源，见 requirements-matrix）；
- 二期三项（R7/R8/R9）不在本批实施，防止签名级变更混入一期纯增量。

---

## 五、执行计划

<!-- 原子提交切分；每步附带验证方式（测试 / 脚本 / 手工冒烟）。 -->

| 步 | 原子提交 | 内容 | 验证方式 |
|---|---|---|---|
| 1 | `refactor(redact): 词表常量化` | R1；签名与语义不变 | 决策表单测；`cargo test --workspace` 全绿即零语义守护；fmt/clippy 零告警 |
| 2 | `feat(redact): RedactionPolicy 与 toml 装配` | R2 + R3 护栏 | 优先级决策表 + "ignore 豁免不了硬键"守护；**反假守护：删护栏分支 → 守护红** |
| 3 | `feat(exporter): 脱敏清单 footer` | R4 | 集成测试：stdout 命令行含 `[REDACTED]` + stderr 清单逐行断言（键名/scheme/长度）；**反假守护：删收集点 → 断言红** |
| 4 | `feat(parser): @sensitive/@no-redact` | R5 | 解析单测 + export/generate 端到端（标注优先级 + 旧文件零回归） |
| 5 | `test(redact): 误报基线与冒烟` | R6 | 内置 ignore 基线测试行；`tests/verify_features.sh` 新步骤：toml 扩展键 → footer 自验闭环 |
| 6 | `docs: 台账登记` | README 导出章节、CHANGELOG、progress_summary 批次行 | 文档一致性核对 |

**回归门禁**（AGENTS §7 三层纪律）：`make gate`（fmt --check → clippy -D warnings → cargo test
--workspace）+ `tests/verify_features.sh` + `examples/run_all.sh`（exporter 行为面变更，全跑）。

---

## 六、评审修订记录（Review Findings & Revisions）

> **评审日期**: 2026-11-10  
> **评审结论**: **Accepted with Architectural Refinements**（评分 9.2/10，6 项架构审查修正全部吸收）

### 6.1 审查发现与吸收清单

| 编号 | 等级 | 维度 | 审查发现 (Finding) | 处置与吸收规格 (Resolution) |
| :---: | :---: | :---: | :--- | :--- |
| **F1** | **P0** | **安全** | **大小写与空白字符规范化盲区**：用户配置 `X-Sentry-Key ` 或 `.http` 写 `@sensitive x-sentry-key`，若未做规范化直接判等，由于 HTTP Header 大小写不敏感（RFC 7230 §3.2），会导致大小写差异漏脱敏。 | **构建期强制规范化**：`RedactionPolicy` 在加载配置与解析标注时，所有 `extra_keys`、`ignore_keys` 必须强制执行 `.trim().to_ascii_lowercase()`；查询匹配输入键时同样小写化比较。 |
| **F2** | **P0** | **安全** | **护栏穿透防呆 (R3 冲突边界)**：若用户配置了 `ignore_keys = ["authorization"]` 或同时把某个键配入 `extra_keys` 与 `ignore_keys`，判定优先级冲突。 | **强制硬键免疫与显式优先级裁决**：<br>1. `EXACT_KEYS`（`authorization`, `cookie` 等）在运行时**绝对不可被 ignore 豁免**，若在 ignore 中声明则打印警告并强行保持脱敏；<br>2. 显式敏感优先：`extra_keys` 优先于 `ignore_keys`，`@sensitive` 优先于 `@no-redact`。 |
| **F3** | **P1** | **架构** | **Clean Architecture 零破坏人体工学 (Zero-Breaking API)**：全库有 5 处直接调用 `is_sensitive_key(key)`。若强制改签名要求传 Policy，会产生大规模破坏性修改。 | **双层 API 设计**：<br>1. 保留 `is_sensitive_key(key: &str) -> bool` 作为快捷纯函数，内部静态委托给 `RedactionPolicy::builtin()`（全库旧调用点 100% 兼容）；<br>2. `RedactionPolicy` 作为一等公民提供 `policy.is_sensitive(key: &str) -> bool` 与 `policy.redact_header(...)`，供 export/generate 按需注入定制策略。 |
| **F4** | **P1** | **性能** | **热点路径零运行时堆分配**：`is_sensitive_key` 在大量 JSON 键和 Header 遍历中高频调用。若在查询中频繁构造字符串或分配 HashSet，会拖慢大响应脱敏速度。 | **静态切片与预处理预编译**：<br>1. 内置 `EXACT_KEYS` 和 `DEFAULT_IGNORE_KEYS` 采用预排序的 `&[&'static str]`，利用二分查找或常量匹配；<br>2. 自定义 policy 内部以 `HashSet<String>` 存储已小写化的键，查询为 $O(1)$，查询过程中**零堆内存分配**。 |
| **F5** | **P1** | **体验** | **Footer 清单管道与脚本纯净性**：`rupost export curl suite.http \| sh` 时，脱敏清单若被非交互式脚本捕获会造成解析崩溃。 | **stderr 专属输出与格式标准化**：<br>1. 脱敏清单强约束仅输出至 `stderr`，`stdout` 100% 保持为原始可执行命令；<br>2. 格式规范化：`[i] Redacted N credential(s): KeyName (Scheme/Opaque, Len)`；`--quiet` 时静音。 |
| **F6** | **P2** | **扩展** | **文件级与请求级标注生命周期**：`.http` 标注 `@sensitive` 与 `@no-redact` 作用域需明确。 | **级联继承模型**：在 `FileMetadata` 挂载文件全局策略，在 `RequestMetadata` 挂载请求局部覆盖；单请求执行与导出时继承合并，局部显式声明优先。 |

