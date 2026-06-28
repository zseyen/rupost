# AGENTS.md - Rupost 智能体协作指南

本项目的智能体协作遵循 **Clean Architecture** 原则，旨在构建一个极致简洁且高度可扩展的 API 文档驱动型测试工具。


Behavioral guidelines to reduce common LLM coding mistakes. Merge with project-specific instructions as needed.

**Tradeoff:** These guidelines bias toward caution over speed. For trivial tasks, use judgment.

## 1. Think Before Coding

**Don't assume. Don't hide confusion. Surface tradeoffs.**

Before implementing:
- State your assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them - don't pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop. Name what's confusing. Ask.

## 2. Simplicity First

**Minimum code that solves the problem. Nothing speculative.**

- No features beyond what was asked.
- No abstractions for single-use code.
- No "flexibility" or "configurability" that wasn't requested.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

Ask yourself: "Would a senior engineer say this is overcomplicated?" If yes, simplify.

## 3. Surgical Changes

**Touch only what you must. Clean up only your own mess.**

When editing existing code:
- Don't "improve" adjacent code, comments, or formatting.
- Don't refactor things that aren't broken.
- Match existing style, even if you'd do it differently.
- If you notice unrelated dead code, mention it - don't delete it.

When your changes create orphans:
- Remove imports/variables/functions that YOUR changes made unused.
- Don't remove pre-existing dead code unless asked.

The test: Every changed line should trace directly to the user's request.

## 4. Goal-Driven Execution

**Define success criteria. Loop until verified.**

Transform tasks into verifiable goals:
- "Add validation" → "Write tests for invalid inputs, then make them pass"
- "Fix the bug" → "Write a test that reproduces it, then make it pass"
- "Refactor X" → "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:
```
1. [Step] → verify: [check]
2. [Step] → verify: [check]
3. [Step] → verify: [check]
```

Strong success criteria let you loop independently. Weak criteria ("make it work") require constant clarification.

---

**These guidelines are working if:** fewer unnecessary changes in diffs, fewer rewrites due to overcomplication, and clarifying questions come before implementation rather than after mistakes.

## 项目愿景

`rupost` 是一个基于文档的 API 测试工具。我们追求在细节中发现美感，通过最简约的设计实现最强大的功能。

## 智能体角色定义

为了实现项目的卓越品质，我们定义了以下核心智能体角色，每个角色对应项目特定的 `.agent/skills`：

### 1. 架构师 (Architect) - Brainstormer
*   **关联技能**: `brainstorming`, `rupost-architecture`, `ai-integration-guidelines`
*   **职责**:
    *   将最初的创意转化为详细的设计方案和规格。
    *   在实施前进行需求分析与架构评审。
    *   平衡理想设计与项目进度，遵循 MVP 准则。
*   **原则**: YAGNI (You Ain't Gonna Need It)，一次只解决一个核心问题。

### 2. Implementation Expert (Rust Async)
*   **关联技能**: `rust-async-patterns`, `rupost-architecture`
*   **职责**:
    *   负责 `rupost` 核心引擎的开发，特别是基于 Tokio 的异步模型。
    *   处理并发网络请求、Cookie 管理、文件执行流。
    *   确保代码符合 Rust 最佳实践，追求高性能与内存安全。
*   **原则**: 遵循开闭原则，确保核心逻辑易于扩展而不必修改已有代码。

### 3. UI/UX 专家 (Frontend Specialist)
*   **关联技能**: `frontend-design`, `cli-aesthetic-ux`
*   **职责**:
    *   设计并实现 `rupost` 的用户界面（如 Dashboard 或示例页面）。
    *   确保界面美观、具有高级感，避免“AI 同质化”审美。
*   **原则**: 极简主义与极致细节，通过优秀的排版、色彩与微交互提升用户体验。

### 4. 协作编排者 (Orchestrator)
*   **关联技能**: `superpower`
*   **职责**:
    *   管理不同智能体之间的协作流程。
    *   确保在每次任务开始前调用正确的技能（Skills）。
    *   维护项目的工作流、文档一致性。

## 协作流程与规则

在 `rupost` 的开发生命周期中，无论是智能体（AI）还是人类开发者，都必须严格遵守并执行 [研发流程规范](file:///Users/zsyzzx/project/rust/rupost/doc/dev/development_workflow.md)。该流程规范由以下核心环节构成：

1. **功能分析与架构设计 (Stage 0 & 1)**：所有新功能或重大变更必须先由 **架构师** 通过 `brainstorming` 流程输出 PRD 与设计文档（存放在 `doc/plans/`），并确保设计符合 **Clean Architecture** 规范。
2. **关键代码与单元测试 (Stage 2 & 3)**：在动工前先设计核心 Trait 与数据模型，单元测试必须在 `mod tests` 编写，并通过 Mock 隔离外部网络与文件 IO。
3. **具体实现与 E2E 校验 (Stage 4 & 5)**：极致简洁实现，利用 `jj` 命令进行原子化提交，并在 `tests/` 下编写真实环境的 E2E 校验。
4. **进度归档与文档更新 (Stage 6)**：将已完成功能记录到唯一的进度事实来源 [progress_summary.md](file:///Users/zsyzzx/project/rust/rupost/doc/plans/progress_summary.md) 中，并同步更新 [README.md](file:///Users/zsyzzx/project/rust/rupost/README.md) 与 `checkpoint.md`。

---


