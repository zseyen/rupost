# RuPost AI 需求汇总与 MVP 计划（Consolidation）

> **创建日期**: 2026-10-27
> **性质**: AI 线**执行计划**——汇总全部需求（含已交付/已立项/效率修正/E 系列/双主体），逐项优劣势分析，收敛出 MVP 范围与三个冲刺。父纲：[总体规划](file:///Users/zsyzzx/project/rust/rupost/.workspace/rupost-ai/doc/plans/2026-10-27-ai-master-roadmap.md)（B0–B4 兜底不变量、效率准入公式）。
> **决策背景**: 用户裁决"当前 AI 接入形式吸引力不足、不能提效"——MVP 必须用可度量的效率证据回应。

---

## 1. 需求汇总（全清单）

### 1.1 已交付（存量，不再投入）

| ID | 需求 | 形态 | 备注 |
| :-- | :--- | :--- | :--- |
| D1 | `ai assert` CLI + 离线 dogfooding + 加固（超时/空 Key/URL） | 命令 | demo 价值已兑现；专家手写更快（效率为负），维持不扩 |

### 1.2 已立项待实施（文档齐备）

| ID | 需求 | 形态 | 文档 | 依赖 |
| :-- | :--- | :--- | :--- | :--- |
| R1 (P0) | `--last/--response` 上下文断言 + **redactor 脱敏器** + 零配置链（env > `~/.rupost/ai.toml` > Ollama 探测） | 命令增强 | PRD + 测试方案 ✅ | 无（纯新增） |
| R2 (Phase 1) | `ai bootstrap --from history`：历史 → 语义增强回归资产（断言/`@capture`/`@depends-on`） | 批量 | 提案 ✅ | **R1 的 redactor 与历史采集** |
| R3 (P2) | OpenAPI 导入 `ai gen`：确定性基线（请求块/mock 骨架/status 断言）+ AI 语义增强 | 批量+确定性 | 开发/测试计划 ✅ | 效率修正后：**确定性先行，AI 部分缓办** |

### 1.3 效率修正派生（就地形态，回应"不提效"批评）

| ID | 需求 | 形态 | AI? |
| :-- | :--- | :--- | :--- |
| R4 | **`import curl`**：浏览器 copy-as-curl → `.http` 请求块 | 就地 | **非 AI**（纯解析器） |
| R5 | TUI 编辑器内嵌断言补全 + E7 body 模版补全（本地 Trie + LLM 兜底） | 就地 | 混合 |
| R6 | `# @ai-assert` 文件内指令 + `rupost ai resolve` 原地展开 | 就地+批量 | AI |
| R7 | **E2 失败归因就地化**：`t` 失败输出末尾 AI 根因假设（`--explain` 显式开启；debug 文档规则版 [Possible Causes] 的升级） | 认知+就地 | AI |
| R8 | **E5 CI 失败聚类 triage**：`ai triage report.json`——确定性签名分组基线 + AI 语义聚类 | 批量认知 | 混合（有确定性基线） |

### 1.4 跟随/远期（依赖基建，登记不排期）

| ID | 需求 | 等待的基建 |
| :-- | :--- | :--- |
| E1/E4 | 快照 diff / 环境差异归因 | 确定性 compare 命令 |
| E6 | spec 变更影响分析 | R3 的 openapi parser |
| E8 | 边界用例生成 | 并入 R3 增强项 |
| E9 | WS 帧序列归因 | 需求验证 |
| E3 | 多源日志根因分析 | 日志集成整套基建（SSH/K8s/ES） |
| 调用侧 agent 化端点（工具循环型 `AiClient`） | E3 深度模式 |

### 1.5 供给侧（双主体战略）

| ID | 需求 | 内容 |
| :-- | :--- | :--- |
| R9 | **MCP Server MVP** | 三 Tool：`run_suite` / `get_history`（脱敏）/ `start_mock`；stdio JSON-RPC；安全边界（工作区 allowlist、mock-only 模式） |

### 1.6 治理（贯穿所有需求）

效率准入公式（省时 > 切换+等待+审查）、B1/B2 兜底声明强制、降级用例强制、五阶段管线模板、README"无 AI 会怎样"条款。

---

## 2. 优劣势分析

### 2.1 逐项

| ID | 优势 | 劣势/风险 | 成本 | 判定 |
| :-- | :--- | :--- | :--- | :--- |
| R1 | 消灭 S0 配置摩擦；**redactor 是后续一切（R2/R9）的复用地基**；探测链让 Ollama 用户零配置 | assert 本身价值天花板低（低频）；探测有本地占位 Key 边界（已设计 TC-C07） | ~3 天 | ✅ MVP（地基属性） |
| R4 | **日常高频、零等待、非 AI 无兜底负担**；与 exporter 互为逆变换（exporter 已有转义逻辑可反推） | curl 语法动物园（shell 引号 `$'..'`、多行、wget 变体）解析长尾；需引 `shell-words` 类小依赖 | ~3 天 | ✅ MVP（对"不提效"最直接的正名） |
| R7 (E2) | 素材零新增（TestResult 已交付）；**就地零切换**；`--explain` 显式开启不破坏离线默认；呼应 debug 文档既有设计 | 失败路径上叠 AI 延迟（本地模型需 ≤10s 否则体验反噬）；输出质量方差 | ~2 天 | ✅ MVP |
| R8 (E5) | QA 最强痛点（小时级→分钟级）；JSON 报告已交付；**确定性签名分组可做离线基线（B1 而非 B2）**；纯读文件不碰网络栈 | 大报告 token 成本（需截断策略）；聚类质量依赖模型 | ~3 天 | ✅ MVP |
| R2 (bootstrap) | 唯一批量形态过线者；竞争空位（vs Keploy 轻量 + Git 原生）；喂 A3/A4 护城河 | `@capture` 链推断的 lint 密度高；依赖 R1 地基 | ~4 天 | ✅ MVP |
| R9 (MCP) | 契约驱动 agent 闭环（叙事+生态双收益）；包壳既有命令新逻辑少；通用 curl MCP server 无契约/断言的空位 | Rust MCP SDK 成熟度；**安全边界第一天就要对**（依赖 R1 redactor）；协议版本维护 | ~4 天 | ✅ MVP（供给侧行雄件） |
| R3 (OpenAPI) | S1 冷启动坡道；确定性部分离线零风险 | **AI 增强部分审查成本 > 增益**（效率修正裁决）；YAML 缺位；长尾 spec 特性 | 4–6 天 | ⏸ 缓办——确定性导入进 fast-follow（与 R4 共享 importer 架构），AI 部分停泊 |
| R5 (TUI 补全) | 就地形态的理想终态 | **TUI 异步改造（mpsc/事件循环）回归风险最大**；工程量高 | ~5 天+ | ⏸ fast-follow（等就地形态用 R6/R7 验证需求） |
| R6 (@ai-assert resolve) | 意图与产物同址、git diff 可审；复用 translator | 需解析器容忍未知指令（触 parser 核心，回归面）；指令命名需设计评审 | ~2 天 | ⏸ fast-follow #1（MVP 后立即） |
| E1/E4/E6/E8/E9/E3 | 各有价值 | 全部被基建依赖挡住（§1.4） | — | 登记跟随 |

### 2.2 组合层分析

- **MVP 六项的互补性**：R1（地基）→ R2/R9（两个消费方）；R4（零 AI 的日常效率）+ R7/R8（认知效率）+ R2（批量效率）——三形态各有代表，"不提效"的批评被三个方向同时回应；R9 独占供给轴。
- **最大的整体风险是范围失控**：六项 ≈ 3 周单人工作量，已到上限。治理条款（准入公式 + 兜底声明 + 降级用例）在 MVP 中首次全面执行，本身就是对治理机制的验证。
- **架构复用红利**：R4 与 R3 共享 `src/importer/`（外部格式 → 文档）；R1 redactor 被 R2/R9 消费；R7/R8 共享 R1 的配置解析与提示惯例——MVP 内部依赖成链，顺序不可乱。

---

## 3. MVP 计划（3 冲刺 / ~3 周）

### 3.1 范围与顺序（依赖驱动）

```
M1 地基与即时效率（~8 天）
   R1 零配置链 + redactor ──→ R4 import curl ──→ R7 E2 失败归因(--explain)
M2 批量与认知（~7 天）
   R8 E5 triage（确定性分组基线先行）──→ R2 bootstrap 核心断言/name/depends_on
M3 供给侧（~5 天）
   R9 MCP MVP：stdio JSON-RPC + run_suite/get_history(脱敏)/start_mock + 安全边界
```

### 3.2 各项验收（效率可度量）

| 项 | 机器可验指标 |
| :-- | :--- |
| R1 | 零配置 Ollama 机器首跑成功；脱敏矩阵全绿；`ai.toml` 含 Key 拒绝 |
| R4 | 20 条真实 devtools curl 语料 fixture 转换全对（含引号/多行/`$'...'`）；转换耗时 < 1s；`--print` 预览与 `>>` 追加两模式 |
| R7 | `--explain` 仅失败时触发；本地模型 ≤ 10s 出假设；默认关闭时行为与今日完全一致（回归锁定）；上下文脱敏（复用 redactor） |
| R8 | 50 条失败种子报告 → 簇摘要 ≤ 60s；无 AI 时确定性签名分组仍产出可用 triage（B1） |
| R2 | 生成物 `rupost t` 断网全绿；AI 不可用降级机械版；防覆盖；lint 丢弃有警告 |
| R9 | 脚本化 agent 闭环：读契约 → start_mock → run_suite → 按结构化报告迭代至绿；history 输出 grep 不到敏感值；allowlist 外 base_url 拒绝 |

### 3.3 MVP 明确不做（防蔓延）

R3 的 AI 部分（停泊）、R5 TUI 补全、R6 resolve（fast-follow #1）、R3 确定性导入（fast-follow #2，复用 importer 架构）、E1/E4/E6/E8/E9/E3、Postman 导入、agent 化端点。

### 3.4 提交与质量门禁

每项独立 jj 提交序列（feat/test/docs 三段式照旧）；提交前三步 + `cargo test --workspace` 全量；`verify_features.sh` 每项新增一步；**每项 PRD 微文档随 feat 提交**（本计划 §2 表即分析底稿，实施时展开为测试方案）。

### 3.5 MVP 成功总标准

1. 效率三方向各有度量证据（§3.2），回应"不提效"批评。
2. 供给侧行雄件落地：外部 coding agent 可经 MCP 消费 rupost 契约。
3. 治理首次全量执行：所有新功能带 B 级声明 + 降级用例。
4. 全量测试 ≥ 600（现 527），零外网闭环（dogfooding 范式贯穿）。

---

## 4. 总体规划执行顺序修订

[总体规划 §4](file:///Users/zsyzzx/project/rust/rupost/.workspace/rupost-ai/doc/plans/2026-10-27-ai-master-roadmap.md) 的阶段表按本计划重排执行序：P0 →（M1：R4/R7）→（M2：E5/bootstrap）→（M3：MCP）→ fast-follow（R6 resolve、R3 确定性导入）→ 候选与跟随批次不变。P2 的 AI 增强部分状态由"已立项"改为"停泊待验证"。
