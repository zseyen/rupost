# RuPost AI 激活 PRD：`rupost ai assert` 自然语言断言生成 (v1.0)

> **创建日期**: 2026-10-26
> **来源**: [竞争格局复审 A1](file:///Users/zsyzzx/project/rust/rupost/doc/competition/2026-10-25-competitive-landscape-review.md)（"最高杠杆动作：激活 rupost-ai"）+ [全文档整合审计 §5.1](file:///Users/zsyzzx/project/rust/rupost/doc/dev/2026-10-26-project-integration-report.md)（"激活前须先做合并/接线架构决策"）
> **阶段**: Stage 0（PRD）+ Stage 1/2（架构决策与关键代码）——遵循 [7-Stage 研发流程](file:///Users/zsyzzx/project/rust/rupost/doc/dev/development_workflow.md)
> **设计期实证**: crate 实际内容已逐行核实，**纠正一处既有文档误判**（见 §2）

---

## 1. 背景与痛点

**现状**：`crates/rupost-ai` 是 workspace 成员但主包零依赖、全库零 `use rupost_ai`——被整合审计定性为"无引用死代码 + 双实现悬空"。竞争复审把激活它列为最高杠杆动作，但激活前必须回答：**这个 crate 的独有价值到底是什么？**

**设计期逐行核实结论（纠正审计与 directory_structure.md 的双重误判）**：

| 组件 | 实际内容 | 与 `src/http/llm_adapter.rs` 的关系 |
| :--- | :--- | :--- |
| `crates/rupost-ai/src/client.rs`（101 行） | `AiClient` trait + `OpenAiClient`——**非流式** chat completion 客户端（prompt→String） | 无重叠（llm_adapter 不做 chat 补全） |
| `crates/rupost-ai/src/translator.rs`（120 行） | `AssertionTranslator`——**自然语言 → RuPost 标准断言**（`"登录成功且返回token"` → `["status == 200", "body.token exists"]`），prompt 固化 8 运算符语法，**已带 3 个 mockall 单测** | 无重叠（llm_adapter 是流式 delta 提取） |
| `src/http/llm_adapter.rs`（58 行） | `LlmStreamAdapter`——URL 探测 provider + SSE delta 提取（已交付、已接线、已测试） | — |

**结论：所谓"双实现悬空"实为两个不同层的职责**（流式运行时 vs 创作辅助），真正的问题是：① crate 的 `AssertionTranslator` 是一个**已写好、已测试、从未接线**的完整用户功能；② `directory_structure.md` 把 translator.rs 错误描述为"流式事件帧规整"（那是 llm_adapter 的职责），文档-代码失真掩盖了 crate 的真实定位。

**用户痛点**：写 `@assert` 需要精确掌握 8 运算符与 JSONPath 语法；新手面对"我想验证登录成功且带 token"到 `@assert body.token exists` 之间有表达鸿沟。RuPost 已是"测 AI 的接口"的工具（A1 已交付），但"AI 帮你写测试"的创作辅助是零成本存量资产。

## 2. 目标与非目标

### 目标（本次交付）
1. **接线激活**：主包依赖 crate，`AssertionTranslator` 从死代码变为用户可感知的命令。
2. **架构决策落地**：以"文档明确的单一职责"终结悬空状态（§3）。
3. **离线自证闭环**：功能可用 `rupost mock` 自身做后端完成端到端验证——**不需要真实 API Key**（dogfooding，同时是项目的最佳广告）。
4. **公开示例 + 注册**：`examples/ai_assert/` + `verify_features.sh` 新步骤。

### 非目标（显式裁剪，遵循 YAGNI）
| 裁剪项 | 理由 |
| :--- | :--- |
| `@ai_assert` 运行时指令 | **违反离线隔离三原则**——测试执行期调用外部 AI 使回归不可复现。明确 Won't do，仅创作期（authoring-time）调用 |
| TUI 集成（`a` 键唤起） | 二期；CLI 命令先行验证需求 |
| 带响应上下文的翻译（`--response`） | 需改 translator 签名与 prompt，MVP 用存量 `translate()` 即覆盖主场景 |
| `rupost ai gen`（生成整个测试文件）、历史→测试建议 | future_backlog 条目 6 保持远期，本次只打地基 |
| `[ai]` 配置节、密钥写入 toml | MVP 仅环境变量（密钥不入盘符合安全直觉）；配置节待有第二个 AI 功能再抽象 |
| 合并/搬移 `llm_adapter.rs` 进 crate | 已交付已测试代码零用户价值搬移 = 纯回归风险；用文档单一职责替代代码搬移（§3） |

## 3. 架构决策：接线 + 文档单一职责（替代方案对比）

| 方案 | 动作 | 评估 |
| :--- | :--- | :--- |
| **A. 接线 + 职责文档化（采纳）** | 主包加 1 行依赖；新增 `ai` 子命令消费 crate；修正 `directory_structure.md` 对 crate 的错误描述；crate 定位明确为 **"AI 创作辅助"**（`AiClient`/`AssertionTranslator`），`llm_adapter` 保持 **"LLM 流量运行时"** | 1 行依赖 + ~60 行 CLI 接线激活已测试资产；零搬移零回归；文档纠错同步完成 |
| B. 合并（llm_adapter 移入 crate） | 统一"所有 AI 相关"到 crate | 已交付 A1 路径代码搬移，无用户价值，回归风险 > 收益。**否决** |
| C. 删除 crate、内联 translator | 甩掉孤儿 crate 名声 | 丢弃已测试资产与 workspace 结构，为"好看"付真实成本。**否决** |

**依赖规则核验**（Clean Architecture）：`main → rupost-ai` 为 Adapter 层对独立 crate 的外向依赖；crate 自身仅依赖 reqwest/serde/async_trait，无反向依赖——分层合规。

## 4. 用户故事与验收标准

### US1：新手一句自然语言拿到可粘贴的断言
> **As a** 刚接触 RuPost 的 API 调试者，**I want** 用一句中文描述期望就直接得到标准断言行，**So that** 不用先学 8 运算符语法就能写出可回归的测试。

- **Given** 环境已配置 `RUPOST_AI_API_KEY`（或 `OPENAI_API_KEY`）与 `RUPOST_AI_BASE_URL`
- **When** `rupost ai assert "登录成功并且响应里包含token"`
- **Then** 终端输出彩色前缀 + 可直接粘贴的断言行：
  ```
  [✓] 已生成 2 条标准断言（粘贴到请求块末尾即可生效）:
  # @assert status == 200
  # @assert body.token exists
  ```
- **Then** 退出码 0；输出不含 API Key、不含 prompt 原文。

### US2：零配置离线体验（dogfooding 闭环）
> **As a** 评估者，**I want** 不申请任何 API Key 就能完整体验 AI 断言功能，**So that** 五分钟内判断这个工具是否值得引入。

- **Given** `examples/ai_assert/mock_ai.json` 提供的 OpenAI 兼容 mock 规则
- **When** 终端 A：`rupost mock examples/ai_assert/mock_ai.json --port 9000`；终端 B：`RUPOST_AI_BASE_URL=http://127.0.0.1:9000 RUPOST_AI_API_KEY=demo rupost ai assert "status is 200 and token exists"`
- **Then** 断言输出正确；`verify_features.sh` 新增步骤复现此闭环并 PASS。

### US3：错误路径可读、密钥不泄露
- **Given** 未设置任何 Key 环境变量
- **When** `rupost ai assert "..."
```
- **Then** 退出码 1，错误信息含可执行指引：`未配置 AI 密钥：export RUPOST_AI_API_KEY=... （或 OPENAI_API_KEY）`。
- **Given** 后端返回 401（mock 场景）
- **When** 同上命令
- **Then** 退出码 1，错误含状态码与截断的响应体；**输出与 `RUST_LOG` 日志均不出现明文 Key**（Authorization 头值不进任何日志路径）。

### US4（命令面细节）
- `rupost ai assert "<text>" --raw`：机器可读模式，仅输出断言行（无前缀无着色），供脚本 `>>` 追加进 `.http` 文件。
- `rupost ai` 无子参数 → clap 自动帮助（列出 assert 用法），不报 panic。
- `--raw` 输出仍以换行分隔多条断言。

## 5. 关键代码设计（Stage 2）

### 5.1 CLI（`src/cli.rs`）
```rust
/// AI assisted authoring commands
Ai {
    #[command(subcommand)]
    command: AiCommands,
},
// ...
#[derive(Subcommand)]
pub enum AiCommands {
    /// Translate natural language expectations into standard @assert lines
    Assert {
        /// Natural language description of expected behavior
        text: String,
        /// Print plain assertion lines only (machine-friendly)
        #[arg(long)]
        raw: bool,
    },
}
```

### 5.2 分发（`src/main.rs`，约 40 行）
```rust
Some(Commands::Ai { command }) => match command {
    AiCommands::Assert { text, raw } => {
        let api_key = std::env::var("RUPOST_AI_API_KEY")
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .map_err(|_| RupostError::Other(未配置密钥指引))?;   // US3
        let base_url = std::env::var("RUPOST_AI_BASE_URL").ok();
        let model = std::env::var("RUPOST_AI_MODEL").ok();
        let client = OpenAiClient::new(api_key, base_url, model);
        let translator = AssertionTranslator::new(std::sync::Arc::new(client));
        let assertions = translator.translate(&text).await?;      // AiError → RupostError 映射
        // raw=false: 彩色 [✓] 前缀 + "# @assert " 行；raw=true: 纯行
    }
},
```

### 5.3 错误映射（`crates/rupost-ai/src/error.rs` 不动；`src/error.rs` 加一个 `From<AiError>` 或分发处 `map_err`，二选一以 clippy 干净为准）

## 6. 测试设计（对齐 [测试方案](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-25-test-strategy-plan.md) L1/L2）

| 层 | 测试 | 场景 |
| :-- | :--- | :--- |
| L1 crate | `client.rs` 0 → 4 测试（crate 加 `wiremock` dev-dep） | 成功解析 / 401 ApiError 带状态码与体 / 空 choices → TranslationFailed / 非 JSON 响应体可读错误 |
| L1 crate | translator 现有 3 测试保持全绿（不改动） | — |
| L2 集成 | 新建 `tests/ai_assert_test.rs`（二进制级，`CARGO_BIN_EXE_rupost`） | ① dogfooding 闭环：子进程起 `rupost mock`（固定 chat completion JSON）→ `ai assert` 经 `RUPOST_AI_BASE_URL` 打回本地 → stdout 含 `# @assert status == 200`；② 缺 Key 退出码 1 + 指引文案；③ mock 返回 401 → 退出码 1 + 含 401 |
| L3 脚本 | `tests/verify_features.sh` 追加步骤 8 | §4-US2 场景复现，随全量脚本回归 |

**mock 配置要点**：`mock_ai.json` 含一条 `POST /v1/chat/completions` 规则，响应体为固定 `{"choices":[{"message":{"content":"[\"status == 200\", \"body.token exists\"]"}}]}`——与 translator 的 JSON 数组解析约定对齐。

## 7. 交付物清单（4 个 jj 原子提交）

| # | 提交 | 内容 |
| :-- | :--- | :--- |
| 1 | `feat(ai): wire rupost-ai into CLI with ai assert command` | Cargo.toml 依赖 + cli.rs/main.rs 分发 + 错误映射（US1/US3/US4 全路径可跑） |
| 2 | `test(ai): crate client tests + offline dogfooding integration test` | §6 全部测试 + `examples/ai_assert/mock_ai.json` |
| 3 | `feat(examples): ai_assert walkthrough + verify_features step` | `examples/ai_assert/README.md`（含 mock 闭环演示）+ verify_features.sh 步骤 8 |
| 4 | `docs: align rupost-ai crate role, register feature` | directory_structure.md translator.rs 描述纠错 + README 特性区 + CHANGELOG Added |

**提交前三步**照旧；提交 2 需加跑 `cargo test --workspace`（首次覆盖 crate）。

## 8. 非功能需求

- **性能**：`ai assert` 为一次性命令，无启动性能预算冲突；不引入新依赖（reqwest/serde/async_trait 复用主包既有版本树，workspace 统一解析）。
- **安全**：Key 仅从环境变量读取，不落盘不入日志；错误信息对响应体做 500 字符截断；`mock_ai.json` 的 `demo` Key 仅存在于本地示例。
- **美感**：输出遵循既有 `[✓]` 高亮惯例；`--raw` 为纯文本——两条输出路径都最小化。

## 9. 成功指标

1. `cargo test --workspace` 全绿且 crate 测试数 ≥ 7（client 4 + translator 3）。
2. `tests/verify_features.sh` 新步骤 PASS——**无真实 API Key 的完整离线闭环**。
3. `grep -r "use rupost_ai" src/` 出现非空命中（死代码状态终结的机器可验证据）。
4. README 特性区新增一行可被新用户 5 分钟内复现（examples/ai_assert）。

## 10. 后续接口（本次不打地基之外的）

- 二期候选：`--response <file>` 上下文感知翻译、TUI `a` 键、`[ai]` 配置节、`rupost ai gen`（生成用例骨架）——全部登记 future_backlog，待 US1 验证需求后启动。
- 竞争叙事同步：激活后 A1 竞争点升级为"**测 AI 的接口 + AI 帮你写测试**"双叙事，README 重排时一并体现。
