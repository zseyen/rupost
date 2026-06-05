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
