# RuPost 文件夹/多文件测试高级控制流 Future Backlog

本文件记录了在 Sprint 2.5 阶段为保障产品核心功能发布而临时裁剪的高级特性。这些特性将在后续版本中依据优先级统一排期实现。

---

## 📋 裁剪特性清单

### 1. 条件执行 (`@skip-if`)
*   **状态**：已完成语法预研，延期实现。
*   **需求描述**：在 CI/CD 中能够根据环境变量、用例执行状态等条件动态跳过特定请求或文件（如生产环境跳过危险请求）。
*   **示例语法**：
    ```http
    ### @skip-if {{env.STAGE}} == "prod"
    DELETE https://api.example.com/dangerous-cleanup
    ```
*   **技术架构要点**：
    *   在解析层定义并解析 `skip_if: Option<String>`。
    *   在执行时，复用 `VariableResolver` 对该表达式进行环境插值。
    *   在 `src/runner/executor.rs` 外部提供独立的表达式求值纯函数，将条件求值与 HTTP 执行逻辑彻底解耦。

### 2. 循环执行 (`@loop`)
*   **状态**：已完成语法预研，延期实现。
*   **需求描述**：对特定写接口进行重复压测，或重复触发以制造测试数据。
*   **示例语法**：
    ```http
    ### @loop 5
    POST https://api.example.com/data/generate
    ```
*   **技术架构要点**：
    *   不修改单次执行的 `execute_one` 接口，在其外部通过平铺循环或并发池调用，最终汇总产生多个测试结果项。

### 3. 可视化 HTML 报告与表现层模板 (Sprint 4)
*   **状态**：延期实现。
*   **需求描述**：提供美观、直观的单页 HTML 测试汇总报告，供 Jenkins / GitHub Pages / 团队内部展示。
*   **技术架构要点**：基于 `handlebars-rust` 或 `tera` 对 `BatchExecutor` 产生的 JSON 进行静态渲染。

---

## 📌 2026-10 竞争格局复审登记（来源：[竞争格局复审](file:///Users/zsyzzx/project/rust/rupost/doc/competition/2026-10-25-competitive-landscape-review.md)）

### 4. gRPC / GraphQL 专用支持 — **Won't do（当前）**
*   **状态**：显式取舍，非遗忘。Posting / Bruno / Hoppscotch / Resterm 已将其做成通用客户端标配，RuPost 定位"终端回归引擎"而非通用客户端，跟进无差异化收益。
*   **重开条件**：若做，优先与流式叙事一致的方向（gRPC-Web / server streaming 场景的剧本断言），复用现有 `AssertTarget` 统一断言体系。

### 5. HTTP/3 支持
*   **状态**：登记观察。xh 与 curl 已支持；RuPost 经 reqwest 的能力边界待确认后再排期。

### 6. `generate` 命令的 AI 延伸（历史 → 测试建议）
*   **状态**：**已正式设计**（2026-10-26，从"远期构想"升级为待实施方案）——演进为 [`rupost ai bootstrap` 自举测试方案](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-26-ai-bootstrap-proposal.md) Phase 1（历史→语义增强测试文件，含脱敏/引擎 lint/降级语义）。前置依赖：AI 激活 Phase 0（[AI 激活 PRD](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-26-ai-activation-prd.md)）。
