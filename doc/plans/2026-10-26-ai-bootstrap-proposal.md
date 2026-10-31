# RuPost AI 自举测试方案 (AI-Bootstrapped API Testing Proposal) v1.0

> **创建日期**: 2026-10-26
> **性质**: 技术提案（Stage 0/1，回答"如何通过 AI 实现 API 的自举测试"）；前置依赖 [AI 激活 PRD](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-26-ai-activation-prd.md)（`rupost ai assert` 与 crate 接线为 Phase 0）
> **定位衔接**: [竞争复审 A1](file:///Users/zsyzzx/project/rust/rupost/doc/competition/2026-10-25-competitive-landscape-review.md)（"测 AI 的接口"）的延伸叙事 + [future_backlog 条目 6](file:///Users/zsyzzx/project/rust/rupost/doc/plans/future_backlog.md)（generate 的 AI 延伸）的正式设计
> **与 Keploy 路线的分界**: Keploy 是"AI 帮你测"（平台化、eBPF 录制、黑盒用例）；本方案是 **"AI 帮你把测试写出来，然后 AI 退场"**

---

## 1. 概念定义：什么是"自举测试"

借编译器自举的类比：**用系统自身运行的副产物作为原料，让 AI 提炼出测试资产；测试资产一旦生成就不再依赖 AI，成为纯确定性的回归资产**。

```
真实调试/测试运行（确定性引擎）
        │  副产物：history（含完整响应体，recorder.rs:22 实证）+ --save-snapshot 快照
        ▼
AI 语义提炼（仅创作期，一次性）
        │  建议断言 / 捕获链 / 命名 / 依赖关系
        ▼
引擎本地校验 + 人工修剪（确定性 + 人）
        │  产物：普通 .http/.md 文件（Git 可 diff）
        ▼
回归资产库（自足，零 AI 依赖）
        └──→ CI 持续运行 rupost t ──→ 失败 → 回到顶部（飞轮）
```

**自举的三条判据**（区别于"AI 每次参与执行"的伪自举）：
1. **生成物自足**：产出的测试用普通 `rupost t` 即可运行，断网可跑（离线隔离三原则不破）；
2. **AI 可替换**：换模型、换本地 Ollama、甚至完全不用 AI（机械还原兜底），管线不塌；
3. **飞轮闭环**：生成的测试跑失败 → 修复 → 新历史 → 更好的下一轮生成素材。

## 2. 素材与能力阶梯（L1→L4，按沉没成本递增）

| 层级 | 素材 | AI 增值 | 状态 |
| :-- | :--- | :--- | :--- |
| **L1** 单点断言 | 用户一句自然语言 | NL → 标准 `@assert` 行 | **已设计**（[AI 激活 PRD](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-26-ai-activation-prd.md)，`rupost ai assert`） |
| **L2** 历史→用例（本方案核心） | `history.jsonl` 条目（含完整请求/响应体） | 从"机械还原"升级为"语义增强"：断言建议、`@capture` 链、`@name`、`@depends-on` 推断 | 本方案 Phase 1 |
| **L3** 快照→Mock 回填 | `--save-snapshot` 套件 | 断言 + 把录制响应反推为 `@mock-when` 规则（"录制即 Mock"AI 化；机械版已存在） | Phase 2（机械版已交付） |
| **L4** 文档/Spec→用例骨架 | OpenAPI / Markdown 设计稿 | happy path + 边界 + 鉴权失败矩阵生成 | 远期（依赖 Spec 摄取，未立项） |

**已核实的素材上限**（设计期实证）：`history/recorder.rs:22` 将完整响应体写入历史（`body: Some(response.body.clone())`，best-effort 无截断）；`SnapshotEntry.response` 恒有全量 body。**结论：L2/L3 的 AI 素材是充分的，但脱敏与截断必须做在"发送给 AI 之前"（§5）**。

## 3. 核心管线：`rupost ai bootstrap`

AI 能力收敛在 `rupost ai` 命名空间（用户心智模型：**带网络依赖的命令只存在于 `ai` 家族**，`t`/`g` 等永远确定性）。

```bash
# 从最近 10 条历史生成语义增强的测试文件
rupost ai bootstrap --from history --last 10 -o regen/auth_flow.http

# 从快照套件生成（含全量响应体素材）
rupost ai bootstrap --from snapshot ./snapshots/login.json -o regen/login.http

# 显式关闭脱敏（自查环境）
rupost ai bootstrap --last 5 --no-mask -o out.http
```

### 管线五阶段

| 阶段 | 确定性 | 内容 | 复用/新增 |
| :-- | :--- | :--- | :--- |
| **A 素材采集** | ✅ | `--from history`：复用 `selector::select_entries(Last(n)/Interactive)`；`--from snapshot`：读 `SnapshotSuite` | 复用 selector/storage；新增 snapshot 采集路径 |
| **B 机械还原** | ✅ | 每条目经 `HttpGenerator` 生成基线请求块（method/url/headers/body 齐备，仅 status 断言） | 复用，**零改动** |
| **C AI 语义增强** | 🤖 | 逐块发送脱敏后的紧凑上下文（§5），索取结构化建议（§4）；可指向本地 Ollama/vLLM（`RUPOST_AI_BASE_URL`）→ **全离线 AI 自举** | 新增 crate `TestSynthesizer`（与 AssertionTranslator 同族） |
| **D 引擎校验 + 落盘** | ✅ | 建议过本地 lint（§4）：非法断言丢弃并警告；`@capture→{{var}}` 引用过 DAG 变量存在性检查；`create_exclusive` 原子落盘**绝不覆盖**已有文件 | 复用 assertion parser / path_guard 模式 |
| **E 回归资产** | ✅ | 产物即普通 `.http`，进 `rupost t` / 批量 DAG / CI，零 AI 运行时 | 存量能力 |

### 降级语义（AI 不参与也不塌）

AI 不可用（无 Key / 网络失败 / 输出不合法）→ 输出 **B 阶段机械还原版** + 显著警告（`[!] AI 增强失败（原因），已输出机械还原版`），退出码 0。机械基线本身已是有用的测试文件——这正是"AI 是增强而非依赖"的自举判据 2。

## 4. AI 契约与引擎校验（AI 建议、引擎裁决）

### 4.1 结构化输出 Schema（单请求块）

```json
{
  "name": "login_success",
  "depends_on": [],
  "assertions": ["status == 200", "body.token exists", "body.role == \"admin\""],
  "captures": ["token from body.token"]
}
```

Prompt 约束（继承 AssertionTranslator 的语法固化风格）：断言仅限 8 运算符 × 4 目标；captures 仅 `header.*`/`body.*`/`regex` 三源；`depends_on` 仅可引用本次批次内已有 name。

### 4.2 本地 lint（引擎裁决，AI 输出永远不可直接信任）

| 校验 | 手段 | 失败处置 |
| :--- | :--- | :--- |
| 断言语法 | 喂给现有 `assertion::parser`（8 运算符解析器本身就是 lint 器） | 丢弃该条 + warnings（复用 `TestResult.warnings` 通道模式） |
| 断言值合理性 | `@assert status == 200` 的值域检查、JSONPath 语法过 `utils/jsonpath` | 同上 |
| capture 变量链 | 生成文件内 `@capture X` 与后文 `{{X}}` 引用闭合 | 断链的 capture 保留、引用处降级警告 |
| `@depends-on` | 仅允许批内 name；成环则丢弃全部依赖边并警告（拓扑自检复用 workflow 思路） | 丢弃 + 警告 |
| 脱敏回扫 | 生成文件内不得含 `{{MASKED}}` 占位符进入断言值 | 该断言丢弃 |

## 5. 数据最小化守护栏（发送给 AI 之前）

历史与快照里是真实流量——**默认脱敏是本方案的合规底线**：

| 项 | 规则 |
| :--- | :--- |
| Header | 白名单子集发送（Content-Type/Accept 等安全头）；`Authorization`/`Cookie`/`Set-Cookie`/`X-Api-*` 值 → `{{MASKED}}` |
| Body 字段 | 键名含 `password/token/secret/key/authorization`（大小写不敏感）的值 → `{{MASKED}}`，**结构保留**（AI 仍能理解 JSON 形状） |
| 体积 | 单请求块上下文截断至 4KB（超限截断并标注 `[truncated]`） |
| 关闭方式 | `--no-mask` 显式关闭（自查环境用），关闭时输出一次性醒目提示 |
| 承诺 | 任何日志路径（tracing/错误信息）不回显请求上下文原文——与 AI 激活 PRD 的密钥承诺同源 |

## 6. 用户故事与验收标准

### US1：调试半小时，收获一套可回归的测试文件
> **As a** 手工调试了登录→建单→查询链路的后端开发者，**I want** 一条命令把刚才的调试历史变成带断言的测试文件，**So that** 今天的调试成果沉淀为明天的回归资产而不只是终端里的转瞬输出。

- **Given** 本机 `history.jsonl` 有最近 3 条相关请求（含响应体），`RUPOST_AI_BASE_URL` 指向本地 Ollama（或 mock）
- **When** `rupost ai bootstrap --from history --last 3 -o regen/flow.http`
- **Then** `regen/flow.http` 生成：3 个请求块各带 `# @name`；登录块后有 `# @capture token from body.token`；后两块的鉴权头渲染 `{{token}}` 且头部有 `# @depends-on`；断言过语法 lint；二次运行同命令**报"文件已存在"不覆盖**（init 防覆盖语义对齐）；`rupost t regen/flow.http` 可直接执行（自足判据 1）。

### US2：无 AI 也能用（降级自证）
- **Given** 未配置任何 AI 环境变量
- **When** `rupost ai bootstrap --from history --last 3 -o out.http`
- **Then** 输出机械还原版（`HttpGenerator` 等价物）+ 黄色 `[!]` 警告说明 AI 未启用；退出码 0。

### US3：AI 输出不合法时引擎兜底
- **Given** mock AI 后端返回含非法断言（`body.users[0] ~= 1`）与合法断言的混合建议
- **When** `rupost ai bootstrap ...`
- **Then** 生成文件仅含合法断言；警告列出被丢弃条目及原因（US3 引擎裁决判据）。

### US4：隐私默认安全
- **Given** 历史中某请求带 `Authorization: Bearer eyJ...` 与 `{"password":"hunter2"}`
- **When** 任意 bootstrap 运行（默认脱敏）
- **Then** 发往 AI 的上下文中两值均为 `{{MASKED}}`；生成文件中不含任何明文密钥；`--no-mask` 时打印一次性醒目风险提示。

### US5：dogfooding 全离线演示
- **Given** `examples/ai_bootstrap/`（mock AI 规则 + 预置历史素材）
- **When** `tests/verify_features.sh` 新增步骤：起 `rupost mock`（OpenAI 兼容固定响应）→ 跑一条测试产生真实历史 → `ai bootstrap` → `rupost t` 执行生成物
- **Then** 全绿。**整条自举链无任何外部依赖**——RuPost 用自己的 mock 测自己的 AI 功能，AI 功能再生成自己的测试。

## 7. 分期路线

| 期 | 交付 | 依赖 | 预估 |
| :-- | :--- | :--- | :--- |
| **Phase 0** | AI 激活：`rupost ai assert` + crate 接线（4 提交） | 已 PRD | 1 天 |
| **Phase 1（本方案核心）** | `ai bootstrap --from history`：A/B/D 阶段全量 + C 阶段 MVP（断言/name/depends_on；captures 视 AI 输出质量） | Phase 0 | 2–3 天 |
| **Phase 2** | `--from snapshot` 采集路径 + L3"录制即 Mock"AI 回填（建议 `@mock-when` 规则草稿） | Phase 1 | 2 天 |
| **Phase 3（远期）** | L4 Spec 摄取（OpenAPI→用例矩阵）；TUI 内 review 面 | 需求验证后立项 | — |

## 8. 测试设计（对齐测试方案分层）

| 层 | 测试 | 要点 |
| :-- | :--- | :--- |
| L1 crate | `TestSynthesizer` mockall 单测 + **lint 器表格驱动测试**（合法/非法断言矩阵、capture 断链、依赖环） | lint 是本方案的信任边界，测试密度最高 |
| L1 | 脱敏器单测：键名大小写/嵌套 JSON/数组内对象/4KB 截断边界 | 数据最小化是合规底线 |
| L2 集成 | `tests/ai_bootstrap_test.rs`（二进制级）：种子历史 → mock AI → 生成 → `rupost t` 生成物全绿（**全链离线**）；AI 500 → 降级路径；非法建议 → lint 兜底；防覆盖 | US1/US2/US3/US5 |
| L3 脚本 | `verify_features.sh` 步骤 9（US5 场景） | 随全量回归 |

## 9. 非目标（显式裁剪）

| 裁剪项 | 理由 |
| :--- | :--- |
| 测试执行期任何 AI 参与（自动修复、自动重试判优） | 破坏确定性执行与离线隔离；失败诊断属"AI 帮测"路线，与本项目分界 |
| AI 输出直接落库/自动提交 | 必经引擎 lint + 人工 `git diff`；无"全自动生成合入"模式 |
| 流量代理实时录制（Keploy eBPF 路线） | 平台化重资产；RuPost 自举素材来自自身运行副产物，零新增基础设施 |
| 云端同步生成历史 | 本地文件即事实源（Bruno 已占的叙事不跟） |
| bootstrap 结果的评分/置信度体系 | YAGNI；warnings + 人工 review 已覆盖 MVP 场景 |

## 10. 成功指标

1. **自足性**：`rupost t <生成物>` 在断网环境全绿（判据 1 的机器可验证据）。
2. **可替换性**：同一素材分别用 mock AI / 真实 API / 本地 Ollama 三种后端产出均有效（判据 2）。
3. **零泄漏**：脱敏单测矩阵全绿；生成文件与发送上下文 `grep` 不到原始密钥值。
4. **飞轮**：examples/ai_bootstrap 演示闭环进入 verify_features.sh 且 PASS。
5. 竞争叙事升级落地：README 呈现"测 AI 的接口 + AI 共建回归资产（运行时零 AI）"双叙事。

---

## 附：一句话回答"如何通过 AI 实现 API 的自举测试"

**让 AI 只做一件事：把系统自己运行时产生的原始交互（历史/快照）提炼成人类可读、Git 可审、语法可 lint 的 `.http` 测试文件；然后 AI 退场，测试以纯确定性运行。** 素材自产（history/snapshot 实证存在且含全量响应体）、提炼有界（脱敏+截断+引擎 lint）、产物自足（离线可跑）、闭环自证（生成物跑失败回流为新素材）——四者齐备即为自举，缺一则退化为"AI 测试生成工具"。
