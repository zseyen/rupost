# Rupost AI 蒸馏功能产品需求文档 (PRD)

| 版本 | 状态 | 作者 | 日期 | 描述 |
| :--- | :--- | :--- | :--- | :--- |
| v0.1 | 草案 | Antigravity | 2026-04-15 | 初始版本：引入 AI 蒸馏与女娲.skills 概念 |

## 1. 项目愿景 (Vision)
`rupost` 不应仅仅是一个 API 调试工具，而应成为开发者的**业务知识合伙人**。通过 **AI 蒸馏 (AI Distillation)** 技术，将碎片化的调试记录、历史报文转化为结构化的 **原子技能 (.skills)** 和 **业务真相 (Truth)**，实现从“文档驱动”到“知识驱动”的跃迁。

## 2. 核心目标 (Goals)
- **离线知识沉淀 (Offline First)**：优先实现从历史执行记录中进行深度分析，降低对实时响应的要求，确保蒸馏质量。
- **知识沉淀自动机**：无需人工编写复杂的测试脚本，自动从执行历史中提取业务流。
- **隐性约束显性化**：自动发现文档中未注明的隐性业务逻辑和“坑位”。

---

## 3. 核心功能需求

### 3.1 非侵入式历史采集器 (Passive Collector)
- **需求描述**：在执行 `.http` 文件或命令行调试时，自动且非侵入地记录请求/响应的全量报文。
- **关键细节**：
    - 支持脱敏处理（自动识别并掩码 Token/PWD）。
    - 存储为本地 JSONL 格式（Clean Architecture 下的持久层实现）。

### 3.2 蒸馏引擎 (Distillery Engine) - [核心]
- **模块 A：原子技能提取 (Atomic Skill Mining)**
    - **逻辑**：识别高密度的请求序列，通过图分析技术确定参数流转方向（Entity Linkage）。
    - **产出**：生成结构化的 `.skills` 文件。
- **模块 B：约束与断言推断 (Constraint Inference)**
    - **逻辑**：对比成功与失败的 Trace，找寻共性特征。例如：识别出“库存不足”并非 404，而是 `code: 10022` 且响应耗时显著增加。
    - **产出**：自动为 API 生成智能断言（Proactive Assertions）。

### 3.3 女娲技能规范 (.skills DSL)
- **需求描述**：定义一种基于 YAML 的、人类可读且 AI 易理解的技能定义格式。
- **层级结构**：
    - `Ability`: 基础 API 调用。
    - `Step`: 带前置依赖和数据抽取的调用原子。
    - `Skill`: 多个步骤组成的闭环业务流程。
    - `Truth`: 针对该技能的专家级建议或隐性约束。

### 3.4 离线分析与知识校验 (Verification Loop)
- **需求描述**：蒸馏产物默认为“草稿”状态，用户可通过 `rupost skills` 面板进行审核、编辑和测试验证。
- **核心逻辑**：
  - **回放验证 (Replay Validation)**：蒸馏出的技能必须能针对当前运行环境成功执行至少一次。
  - **变更审计**：记录技能的演进历史，支持追溯到是哪一段历史 Trace 导出了该知识。

### 3.5 上下文关联增强 (Context Enrichment)
- **需求描述**：蒸馏引擎在分析报文的同时，应自动读取相关联的 `.http` 文件、Git 提交记录和 Swagger 文档。
- **目标**：赋予 AI 更多的“背景知识”，使技能命名（如 `apply_discount`）更符合团队命名习惯。

---

## 4. 业务场景举例 (Use Cases)

### 场景一：新员工快速上手
- **现状**：新员工不知道如何构造复杂的支付测试数据。
- **需求实现**：执行 `rupost test Payment.skills --mock-user`。AI 自动根据蒸馏出的逻辑，依次调用 Login, CreateOrder, Pay 接口，屏蔽底层参数拼接细节。

### 场景二：生产环境异常复现
- **现状**：生产环境报错 500，开发环境调不通。
- **需求实现**：将生产 Trace 丢给蒸馏引擎，AI 发现差异：“生产环境里 `location_id` 必须是 8 位，而开发环境只有 4 位”。该知识被存入 `Location.skills` 的 `Truth` 模块。

---

## 5. 技术架构原则 (Architecture)

1. **Clean Architecture 遵循**：
    - **Entities**: Skill, Step, Trace, Truth。
    - **Use Cases**: DistillHistory, ExecuteSkill, ValidateConstraint。
    - **Adapters**: HTTP Runner, File System Storage, LLM API Client。
2. **极简主义设计**：
    - `.skills` 文件存储在本地 `.agent/skills/` 目录下，支持 Git 版本管理。
    - 蒸馏过程支持完全离线（手动打标）或半自动（通过 DeepSeek/Gemini 等 LLM）。

## 6. MVP 路线图 (Roadmap)
- **Phase 1 (Basic)**: 实现 Trace 记录器及基础的数据脱敏，能够将简单的线性 API 调用序列转为 `.skills`。
- **Phase 2 (Pro)**: 引入 LLM 识别复杂的参数依赖关系（JSON Path 自动映射），支持智能断言推断。
- **Phase 3 (Enterprise)**: 实现“业务真相”知识库，具备跨团队协同的技能库管理及避坑提醒。
