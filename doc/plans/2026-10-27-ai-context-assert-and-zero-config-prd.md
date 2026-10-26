# RuPost AI 迭代 PRD：上下文感知断言与零配置接入 (v1.0)

> **创建日期**: 2026-10-27
> **阶段**: Stage 0（PRD）——遵循 [7-Stage 研发流程](file:///Users/zsyzzx/project/rust/rupost/doc/dev/development_workflow.md)
> **前置**: [AI 激活 PRD](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-26-ai-activation-prd.md) Phase 0（`rupost ai assert`，已交付）
> **范围**: ① `ai assert --last N` / `--response <file>` 上下文感知断言；② 零配置接入链（环境变量 > 用户级配置 > 本地 Ollama 探测）

---

## 1. 背景与动机

Phase 0 交付的 `rupost ai assert` 打通了"自然语言 → 标准断言"的创作期通路，但试用反馈暴露两个摩擦：

1. **描述盲区**：用户没见过响应结构时，自然语言容易写偏（说"返回 token"，实际在 `body.data.access_token`）——AI 看不到真实响应，只能凭空猜 JSON 路径。
2. **接入仪式感**：使用前需要 `export` 1–3 个环境变量；本机明明跑着 Ollama 的用户也要先配 `RUPOST_AI_BASE_URL`。操作摩擦直接劝退评估者。

本迭代消除这两类摩擦，同时为 [bootstrap Phase 1](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-26-ai-bootstrap-proposal.md) 预制两个可复用组件：**脱敏截断器**与**历史采集通道**。

## 2. 用户画像分析

| 画像 | 一句话速写 | 核心诉求 | 本迭代兑现 |
| :-- | :-- | :-- | :-- |
| **A 新手调试者** | 从 Postman/curl 转来，不懂 8 运算符语法，中文思维 | 把"我想验证什么"直接变成测试 | AI 看见真实响应，断言精确到实际 JSON 路径，无需先手动查看响应 |
| **B 零门槛评估者** | 终端工具榜发现 rupost，不想注册云 AI，本机常已装 Ollama | 5 分钟内完整体验价值 | 零配置探测：本机 Ollama 在跑就直接可用，真·开箱即用 |
| **C 隐私敏感内网用户** | 企业禁止数据出网，用本地/内网 Ollama 或 vLLM | AI 辅助但数据不出内网 | 用户级配置一次生效；上下文默认脱敏 + 截断 |
| **D 回归资产建设者** | 后端开发者，今天手工调试，明天想要回归套件 | 调试成果沉淀为资产 | 本迭代是 bootstrap Phase 1 的铺路石（脱敏器/历史通道直接复用） |

### 2.1 画像 A：新手调试者（"语法焦虑"型）

*   **行为特征**：刚接触 rupost；能跑通请求但写不出 `@assert body.data.role == "admin"`；习惯用中文描述期望。
*   **关键痛点**：Phase 0 解决了"表达"（说人话 → 标准断言），但没解决"描述盲区"——不知道响应长什么样时，说出来的话本身就偏离真实结构，AI 只能生成泛化断言（`body.token exists`）甚至错误路径。
*   **本迭代价值**：`--last 1` 把刚发生的真实响应（脱敏后）交给 AI，断言从"猜"变成"看"。新手的工作流从"跑请求 → cat 响应 → 组织语言 → ai assert"缩短为"跑请求 → `ai assert --last 1` 说话"。

### 2.2 画像 B：零门槛评估者（"五分钟尝鲜"型）

*   **行为特征**：README/口碑驱动而来；评估耐心以分钟计；环境里往往已经有 Ollama（开发机常见）。
*   **关键痛点**：Phase 0 的最小可用路径仍要求"先配置再使用"——`export RUPOST_AI_API_KEY=...` 这一步对没有云 Key 的评估者是硬门槛，流程一旦卡住即流失。
*   **本迭代价值**：探测链让"什么都没配"成为**可用状态**而非错误状态。装了 Ollama 的机器上 `rupost ai assert "..."` 直接出结果，并打印一次性提示告知所用后端——评估者无需阅读任何文档。

### 2.3 画像 C：隐私敏感内网用户

*   **行为特征**：企业合规禁止业务数据出网；使用本地 Ollama 或内网 vLLM；对"响应体被发送到哪"高度敏感。
*   **关键痛点**：① 每个新 shell 都要重新 `export RUPOST_AI_BASE_URL`；② `--last` 引入"发送历史数据给 AI"的新路径，若不默认脱敏，历史响应中的密钥/业务数据会随上下文外流。
*   **本迭代价值**：① 用户级配置 `~/.rupost/ai.toml`（不入项目仓库、不入版本控制）一次配置永久生效；② 上下文默认脱敏（与 [bootstrap 提案 §5 数据最小化守护栏](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-26-ai-bootstrap-proposal.md)同源规范）+ 4KB 截断；③ 密钥承诺延续 Phase 0——任何配置路径都不落盘 Key（用户级配置文件仅允许存 base_url/model，Key 仍只走环境变量）。

### 2.4 画像 D：回归资产建设者（自举用户，前瞻）

*   **行为特征**：手工调试半小时后想把成果变成回归测试；关注"生成的资产是否离线可跑、Git 可审"。
*   **与本迭代的关系**：不是本迭代直接服务对象，但本迭代落地的**脱敏截断器**与**历史最近 N 条读取**正是 bootstrap Phase 1 的 A 阶段（素材采集）与 §5（数据最小化）的复用件——避免 Phase 1 重复造轮子。

## 3. 场景说明（用户故事与验收标准）

### US1：上下文感知断言（画像 A/C）

> **As a** 刚跑完登录请求的调试者，**I want** AI 看着真实响应帮我写断言，**So that** 断言精确到实际 JSON 结构而不是凭空猜测。

*   **Given** 刚执行过 `rupost t login.http`，history 最新条目响应为 `{"data":{"token":"eyJ...","role":"admin"}}`；本机 Ollama 运行中或已配置任一后端
*   **When** `rupost ai assert --last 1 "验证登录成功且返回管理员身份"`
*   **Then** 输出断言精确反映真实结构（如 `# @assert body.data.role == "admin"`、`# @assert body.data.token exists`）；发送给 AI 的上下文中 `token` 值为 `{{MASKED}}`；退出码 0。

### US2：显式响应文件（画像 A）

*   **Given** 响应体已保存为 `resp.json`（无历史或想指定非最近请求）
*   **When** `rupost ai assert --response resp.json "验证分页字段齐全"`
*   **Then** 行为与 US1 等价（同样脱敏 + 截断）；`--last` 与 `--response` 互斥，同给报参数冲突。

### US3：零配置本地探测（画像 B/C）

*   **Given** 本机 Ollama 运行在 `localhost:11434`；未设置任何 `RUPOST_AI_*` / `OPENAI_API_KEY`；无用户级配置文件
*   **When** `rupost ai assert "status is 200"`
*   **Then** 命令直接成功；输出含一次性提示（如 `[i] 未检测到显式 AI 配置，已使用本地 Ollama (http://localhost:11434/v1)`）；模型从探测到的可用列表选取。

### US4：探测失败回退可读指引（画像 B）

*   **Given** 无 Ollama、无环境变量、无用户级配置
*   **When** `rupost ai assert "..."`
*   **Then** 退出码 1，错误指引更新为三选一的可执行指引：环境变量 / `~/.rupost/ai.toml` / 安装并启动本地 Ollama。

### US5：配置优先级与安全边界（画像 C）

*   **Given** 环境变量、`~/.rupost/ai.toml`、本地 Ollama 三者同时存在
*   **When** 任意 `ai` 命令运行
*   **Then** 解析优先级 **环境变量 > 用户级配置 > 本地探测**；`ai.toml` 中**不允许也不读取** api_key 字段（Key 只走环境变量的承诺不破）；项目级 `rupost.toml` 不参与 AI 配置。

### US6：上下文数据最小化（画像 C，红线场景）

*   **Given** 历史响应含 `Authorization: Bearer eyJ...` 头与 `{"password":"hunter2","data":{...}}` 体
*   **When** 任意带 `--last`/`--response` 的 `ai assert` 运行（默认脱敏）
*   **Then** 发送上下文中上述值均为 `{{MASKED}}`（JSON 结构保留，AI 仍可理解形状）；单条上下文截断至 4KB 并标注 `[truncated]`；`--no-mask` 显式关闭时打印一次性醒目风险提示；输出与日志不回显上下文原文。

## 4. 目标与非目标

### 目标

1. `ai assert` 新增 `--last N` 与 `--response <file>`（互斥），上下文默认脱敏 + 4KB 截断。
2. 零配置接入链：环境变量 > `~/.rupost/ai.toml`（仅 base_url/model）> 本地 Ollama 探测（`GET /v1/models`）。
3. 脱敏截断器与历史采集实现为可复用组件（bootstrap Phase 1 直接复用）。
4. 探测成功时输出一次性后端提示；失败时回退三选一可执行指引。

### 非目标（显式裁剪，YAGNI）

| 裁剪项 | 理由 |
| :-- | :-- |
| `AssertionTranslator` 签名重构 | 响应上下文以文本拼装进 user prompt（"期望 + 实际响应(已脱敏)"），crate 仅微调 system prompt 一句话；结构化上下文等 bootstrap 有真实需求再抽象 |
| 历史交互式选择（TUI picker） | `--last N` 取最近 N 条已覆盖主场景，交互选择留待需求验证 |
| `[ai]` 项目级 toml 配置节 | 密钥安全边界要求 AI 配置不入项目仓库；用户级文件已满足便利性 |
| bootstrap / `ai gen` / TUI `a` 键 / MCP | 各自按既定路线分期，见 [第一阶段规划](file:///Users/zsyzzx/project/rust/rupost/doc/ai/development_roadmap.md) 与 [bootstrap 提案](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-26-ai-bootstrap-proposal.md) |
| 多候选模型自动择优 | 探测取列表首个可用模型即可；择优是伪需求 |

## 5. 方案概要

```
rupost ai assert --last 1 "验证登录..."
        │
        ▼
[配置解析] env(RUPOST_AI_*/OPENAI_API_KEY) > ~/.rupost/ai.toml > 本地探测(GET /v1/models)
        │                                  │主包新增 src/ai/config.rs
        ▼
[素材采集] --last N → history storage 最近 N 条(复用 Phase 0 已实证的全量 body)
        │            --response <file> → 读文件          │src/ai/context.rs
        ▼
[数据最小化] 头白名单 + 敏感键名 {{MASKED}} + 4KB 截断      │src/ai/redactor.rs（Phase 1 复用件）
        ▼
[拼装 user prompt] "期望: <自然语言>\n\n实际响应(已脱敏): <上下文>"
        │  （translator 签名不变，crate 仅微调 system prompt）
        ▼
[AssertionTranslator] → 标准 @assert 行 → 彩色 / --raw 输出
```

**分层合规**：crate 依旧不感知 history 与文件系统（素材在主包侧完成采集与脱敏后才进入 crate）；脱敏在**发送之前**完成，与 bootstrap 提案的"数据最小化做在发送前"承诺同源。

## 6. 测试设计（对齐 L1/L2 分层）

| 层 | 测试 | 场景 |
| :-- | :--- | :--- |
| L1 主包 | `redactor` 单测矩阵 | 键名大小写/嵌套 JSON/数组内对象/头白名单/4KB 截断边界 |
| L1 主包 | `config` 解析单测 | 优先级覆盖矩阵；`ai.toml` 含 api_key 字段时**拒绝并警告**（安全边界） |
| L2 集成 | `tests/ai_assert_test.rs` 扩充 | ① `--last 1` dogfooding：mock 响应含敏感键，断言输出结构精确 + 上下文含 `{{MASKED}}`；② `--response` 文件路径；③ 探测链——**tempdir 重定向 HOME** 写入用户级配置后零环境变量可用（规避 11434 端口依赖）；④ 探测失败回退指引 |
| L3 脚本 | `verify_features.sh` 步骤 7 扩充 | 零 Key 场景追加 `--last 1` 闭环（mock 一条真实请求 → 上下文断言） |

## 7. 交付计划（3 个 jj 原子提交）

| # | 提交 | 内容 |
| :-- | :--- | :--- |
| 1 | `feat(ai): context-aware assert with --last/--response and redaction` | `src/ai/{config,context,redactor}.rs` + cli 参数 + prompt 拼装 |
| 2 | `test(ai): redaction matrix and zero-config integration tests` | §6 全部测试 |
| 3 | `docs(ai): zero-config walkthrough and verify step` | examples/ai_assert README 增补 + verify_features.sh 步骤扩充 |

提交前三步照旧（fmt / clippy -D warnings / cargo test --workspace）。

## 8. 成功指标

1. 装有 Ollama 且零配置的机器上，`rupost ai assert "..."` 首次运行即成功（US3，机器可验）。
2. `--last 1` 生成的断言引用真实响应结构（集成测试断言 `body.data.*` 级路径存在）。
3. 脱敏矩阵全绿：发送上下文与终端输出 `grep` 不到原始密钥值（US6）。
4. `redactor` 与历史采集作为独立模块被 Phase 1 bootstrap 直接 `use`，零复制粘贴（架构验证点）。
