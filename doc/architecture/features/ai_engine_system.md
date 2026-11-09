# AI 智能赋能体系全貌 (AI Intelligence & Assistant Engine)

本文档是 RuPost 项目关于 AI 创作辅助、自然语言断言生成、测试失败智能归因与批量分诊的权威技术档案。整合自早期 AI 可行性报告、方案 A 嵌入式实施方案以及 18 篇演进 PRD，全面总结**战略需求、架构决策演进、当前代码落地现状、后续演进路线**。

---

## 1. 业务需求与战略定位 (Why & What)

### 1.1 核心痛点与行业空白
在终端 API 测试工具领域（如 Hurl、HTTPie、curl、Postman 等），传统测试编写与维护面临重度手工劳作：
1. **断言编写门槛高**：开发者需要熟记各种 JSONPath 语法、软匹配操作符及边界判断逻辑。
2. **测试失败排查成本巨大**：CI/CD 跑出数十个失败用例时，开发者需要逐个查看响应内容、排查是“业务代码改动（预期变化）”、“后端服务宕机（环境故障）”还是“测试用例过期（契约失效）”。
3. **行业差异化定位**：Postman、Bruno、Hurl 等竞品在终端 AI 自动化断言和失败归因领域均处于早期空白。RuPost 战略聚焦于成为**“面向实时与智能化时代的终端回归引擎”**。

### 1.2 核心用户场景与交互契约
- **场景 1：自然语言翻译断言 (`rupost ai assert`)**：
  在 `.http` 文件中编写自然语言注释，AI 自动将其转换为严格的语法结构断言：
  ```bash
  rupost ai assert "响应状态码为 200，且 data.user.id 等于 1001"
  # 输出:
  # HTTP/1.1 200 OK
  # jsonpath "$.data.user.id" == 1001
  ```
- **场景 2：测试失败就地归因 (`--explain`)**：
  用例断言失败时，AI 分析请求、期望与实际 Response，即时给出可读归因报告与修复指引。
- **场景 3：批量失败智能分诊 (`rupost ai triage`)**：
  面对大规模 CI 失败日志，基于特征指纹（Signature Bucket）自动聚类并生成分诊报告，区分环境瞬断、数据不一致或逻辑缺陷。

---

## 2. 架构演进与方案设计抉择 (Architecture & Evolution)

```mermaid
flowchart TD
    subgraph UserInterface ["CLI / TUI 交互层"]
        CmdAssert["rupost ai assert <PROMPT>"]
        CmdTriage["rupost ai triage <REPORT>"]
        FlagExplain["rupost test --explain"]
    end

    subgraph CoreBridge ["rupost 核心调度适配"]
        CliDispatch[CLI 子命令分派]
        TestRunner[Runner 执行引擎]
    end

    subgraph RupostAiCrate ["crates/rupost-ai (独立 Crate)"]
        AiClient[AiClient / OpenAiClient]
        Translator[AssertionTranslator (自然语言转断言)]
        Attributor[FailureAttributor (就地归因)]
        Synthesizer[TestSynthesizer (测试用例合成)]
        TriageEngine[FailureClassifier / TriageReport (聚类分诊)]
    end

    CmdAssert --> CliDispatch
    CmdTriage --> CliDispatch
    FlagExplain --> TestRunner

    CliDispatch --> Translator
    CliDispatch --> TriageEngine
    TestRunner --> Attributor

    Translator --> AiClient
    Attributor --> AiClient
    Synthesizer --> AiClient
    TriageEngine --> AiClient
```

### 2.1 架构方案选型：Scheme A (嵌入式) vs Scheme B (外部 Sidecar)
在早期技术评估中，团队深入对比了两种方案：
*   **Scheme B (外部微服务 / Sidecar 代理)**：
    将 AI 能力做成单独的常驻后台服务，CLI 通过 gRPC/HTTP 调用。
    *缺陷*：安装部署复杂，破坏了 RuPost “单个静态二进制即开即用”的极简美学。
*   **Scheme A (嵌入式独立 Crate，采纳方案)**：
    创建独立的子 Crate [`crates/rupost-ai`](file:///Users/zsyzzx/project/rust/rupost/crates/rupost-ai)，主程序按需依赖编译。
    *收益*：零外部进程依赖，符合 Clean Architecture 原则，将外部 LLM 接口与模型调用完全封装在外部适配层。

### 2.2 核心算法与机制设计

#### 2.2.1 失败分类与指纹聚类算法 (Failure Clustering)
在大规模用例失败时，`crates/rupost-ai/src/triage.rs` 实现了确定性指纹聚类引擎：
1. **特征提取**：从失败用例中抽取 HTTP 状态码、错误类型（网络断开/断言失败/超时）、报错 JSONPath、Target 域名。
2. **Signature Bucket 归一化**：将非决定性变量（如随机 ID、动态时间戳）脱敏，生成标准签名哈希。
3. **分层聚类**：将成百上千条失败划分为几类典型模式（`FailureCategory`），生成包含影响范围、推测原因与建议排查动作的结构化 `TriageReport`。

#### 2.2.2 上下文剪裁与 Prompt 模板工程
为了保证响应速度并控制 Token 成本：
*   **AST 级上下文剪裁**：只提取当前断言失败相关的请求头和响应体片段，大体积响应（如超过 10KB 的 JSON）自动执行语义折叠，保留 Schema 与命中路径上下文。
*   **结构化输出保障 (Structured Output)**：Prompt 采用 JSON 模式约束，配合严密的后处理校验，杜绝模型幻觉产生非法 AST。

---

## 3. 目前已完成什么 (Status & Implementation)

| 核心组件 | 代码位置 | 职责说明 |
| :--- | :--- | :--- |
| **AI 独立 Crate** | [`crates/rupost-ai/`](file:///Users/zsyzzx/project/rust/rupost/crates/rupost-ai) | 承载所有 AI 领域模型、客户端通信与提示词工程，保持主核心纯粹 |
| **断言翻译器** | [`crates/rupost-ai/src/translator.rs`](file:///Users/zsyzzx/project/rust/rupost/crates/rupost-ai/src/translator.rs) | `AssertionTranslator`：自然语言意图转 RuPost 标准断言语法 |
| **失败归因引擎** | [`crates/rupost-ai/src/attribution.rs`](file:///Users/zsyzzx/project/rust/rupost/crates/rupost-ai/src/attribution.rs) | `FailureAttributor`：单请求断言失败就地归因与修复指引生成 |
| **用例合成器** | [`crates/rupost-ai/src/synthesizer.rs`](file:///Users/zsyzzx/project/rust/rupost/crates/rupost-ai/src/synthesizer.rs) | `TestSynthesizer`：基于接口契约或样例响应反向合成测试套件 |
| **聚类与分诊引擎** | [`crates/rupost-ai/src/triage.rs`](file:///Users/zsyzzx/project/rust/rupost/crates/rupost-ai/src/triage.rs) | `FailureClassifier` 与 `TriageReport`：大批量测试失败指纹分类与报告聚合 |
| **LLM 客户端适配** | [`crates/rupost-ai/src/client.rs`](file:///Users/zsyzzx/project/rust/rupost/crates/rupost-ai/src/client.rs) | 基于 `reqwest` 的兼容 OpenAI / DeepSeek / 通义千问的统一 Client 抽象 |
| **CLI 接线分派** | [`src/cli.rs`](file:///Users/zsyzzx/project/rust/rupost/src/cli.rs)<br>[`src/main.rs`](file:///Users/zsyzzx/project/rust/rupost/src/main.rs) | 接入 `rupost ai assert`、`rupost ai triage` 与 `--explain` 命令通道 |

### 验证与测试套件
- **单元测试**：`crates/rupost-ai` 内置完整单元测试，验证了 JSON 容错解析、聚类桶归一化算法与 Mock LLM 交互。
- **集成测试**：[`tests/ai_integration_test.rs`](file:///Users/zsyzzx/project/rust/rupost/tests/ai_integration_test.rs) 验证了端到端命令行调用与错误边界保护。

---

## 4. 后续演进规划与待办 (Roadmap & Backlog)

1. **[P1] 消除重复实现与适配器收敛**：目前 `crates/rupost-ai/src/client.rs` 与主库 `src/http/llm_adapter.rs` 存在部分重叠，后续需彻底下沉收敛为单点实现。
2. **[P2] 本地离线小模型适配 (Ollama / Llama.cpp)**：支持局域网或离线无外网环境下的零门槛推理，保护金融与内网接口敏感数据。
3. **[P2] OpenAPI 自动生成与智能蒸馏**：基于 `doc/plans/2026-10-27-ai-openapi-gen-plan.md`，从 Swagger/OpenAPI 3.x 规格中一键生成覆盖全分支的 `.http` 测试用例集。
4. **[P3] TUI AI 侧边栏助手**：在 TUI 界面中增加即时聊天与断言辅助抽屉，支持对话式调整请求参数。

---

## 5. 历史方案与设计文档追溯 (Historical Archive)

本档案完全吸收并承接了以下历史文档的核心内容与推演细节：
- [`development_roadmap.md` (doc/ai)](../legacy_research/ai/development_roadmap.md)：早期 AI 开发计划与架构演进文档。
- [`feasibility_report.md` (doc/ai)](../legacy_research/ai/feasibility_report.md)：AI 接入可行性分析报告。
- [`implementation_plan_scheme_a.md` (doc/ai)](../legacy_research/ai/implementation_plan_scheme_a.md)：方案 A 嵌入式实施方案详解。
- [`technical_proposals.md` (doc/ai)](../legacy_research/ai/technical_proposals.md)：AI 接入技术方案与架构设计。
- `doc/plans/` 中 18 篇历史 AI 方案（包含 `2026-10-26-ai-activation-prd.md`、`2026-10-27-ai-context-assert-*`、`2026-10-28-ai-failure-triage-*` 等）。
