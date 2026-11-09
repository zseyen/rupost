# RuPost 文件夹测试执行 技术方案 (Folder Execution Plan)

## 1. 概述
本方案旨在实现 RuPost 对一个目录（及其子目录）下所有 `.http` 文件的批量执行能力。这是从"单请求调试"走向"API 自动化测试套件"的关键一步。

**核心价值**: 用户只需运行 `rupost test ./api_tests/`，即可自动发现、执行、报告该目录下所有的 API 测试。

## 2. 功能规格 (Specs)

### 2.1 执行方案 (Execution Modes)

#### 5.1.1 顺序执行 (Sequential)
*   **默认行为**。按照文件名的字典序 (alphabetical order) 遍历执行。
*   **CLI**: `rupost test ./folder --mode sequential`
*   **适用场景**: 存在强依赖的测试流程（如 `01_login.http` -> `02_get_profile.http`）。

#### 5.1.2 并行执行 (Parallel)
*   利用 Tokio 的 `JoinSet` 或 `buffer_unordered` 并发执行。
*   **CLI**: `rupost test ./folder --mode parallel --concurrency 4`
*   **适用场景**: 独立的、无状态的 API 测试（如健康检查、幂等读操作）。

### 2.2 执行顺序处理 (Control Flow)

#### 5.2.1 依赖处理 (Dependencies)
*   **语法**: 在 `.http` 文件头部声明依赖。
    ```http
    ### @depends-on login.http
    GET https://api.example.com/profile
    Authorization: Bearer {{token}}
    ```
*   **逻辑**: 执行引擎会构建 DAG (有向无环图)。
    1.  扫描所有文件，提取 `@depends-on` 关系。
    2.  拓扑排序 (Topological Sort)。
    3.  如果检测到循环依赖，报错并终止。

#### 5.2.2 循环处理 (Looping)
*   **语法**: 支持对单个请求或整个文件进行循环。
    ```http
    ### @loop 3
    POST https://api.example.com/stress
    ```
*   **适用场景**: 简单的压力测试或重试逻辑。

#### 5.2.3 条件处理 (Conditional Execution)
*   **语法**: 基于环境变量或前序结果跳过执行。
    ```http
    ### @skip-if env.CI == "true"
    DELETE https://api.example.com/dangerous
    ```
*   **逻辑**: 在执行前评估条件表达式。若为 `true`，则跳过该请求，状态标记为 `Skipped`。

### 2.3 执行结果 (Results)

#### 5.3.1 跨文件变量共享 (Cross-File Variables)
*   **问题**: `01_login.http` 获取的 `token` 如何传递给 `02_profile.http`？
*   **方案**: 引入 **Global Scope**。
    *   脚本中使用 `rupost.global.set("token", value)` 设置。
    *   后续文件中使用 `{{global.token}}` 引用。
    *   **生命周期**: 整个 `rupost test` 命令执行期间有效，命令结束后销毁。

#### 5.3.2 报告 (Reporting)
*   **输出格式**:
    *   **Terminal (默认)**: 类似 `cargo test` 的简洁输出 (`ok`, `FAILED`, `skipped`)。
    *   **JSON**: `--report json` -> 输出结构化结果，供 CI/CD 解析。
    *   **HTML**: `--report html` -> 生成美观的 HTML 报告（依赖 Phase 3 的模板引擎）。
*   **内容**:
    *   总执行数、成功数、失败数、跳过数。
    *   每个请求的耗时 (Duration)。
    *   失败请求的 Diff (Expected vs Actual Response)。

## 3. 技术架构

```mermaid
graph TD
    CLI[rupost test ./folder] --> Scanner[File Scanner]
    
    subgraph "Execution Engine"
        Scanner -->|.http files| DependencyResolver[Dependency Resolver (DAG)]
        DependencyResolver --> Scheduler{Scheduler}
        
        Scheduler -->|Sequential| SeqRunner[Seq Runner]
        Scheduler -->|Parallel| ParRunner[Parallel Runner (JoinSet)]
        
        SeqRunner --> Executor[Request Executor]
        ParRunner --> Executor
    end
    
    Executor -->|Result| Aggregator[Result Aggregator]
    Aggregator --> Reporter[Reporter]
    
    Reporter -->|Terminal| StdOut
    Reporter -->|JSON| FileJSON[report.json]
    Reporter -->|HTML| FileHTML[report.html]
```

## 4. 详细测试用例 (Test Cases)

#### [Test-Folder-01] 依赖顺序
*   **场景**: 目录下有 `a.http (depends b)`, `b.http`, `c.http (depends a)`。
*   **验证**: 执行顺序必须是 `b -> a -> c`。

#### [Test-Folder-02] 循环依赖检测
*   **场景**: `a.http (depends b)`, `b.http (depends a)`。
*   **验证**: 程序应抛出 `Error: Cyclic dependency detected between a.http and b.http`。

#### [Test-Folder-03] 并行隔离
*   **场景**: 两个文件并行执行，各自设置同名变量 `{{id}}`。
*   **验证**: 变量应互不影响（作用域隔离），除非使用 `global`。

#### [Test-Folder-04] 失败中断 vs 继续
*   **场景**: 3 个文件，第 2 个失败。
*   **CLI**: 
    *   `--fail-fast`: 第 2 个失败后立即停止，不执行第 3 个。
    *   (默认): 继续执行第 3 个，最终报告汇总。

## 5. 风险点与注意事项

1.  **并行 + Cookie 冲突**: 如果多个并行请求共享同一个 Cookie Jar，可能导致数据竞争。
    *   **对策**: 并行模式下，每个 Worker 使用独立的 `CookieStore` 副本，或禁用 Cookie 共享。
2.  **全局变量线程安全**: `rupost.global` 必须是 `Arc<Mutex<Map>>` 或使用无锁的 `DashMap`。
3.  **超大目录**: 如果目录下有 10,000 个 .http 文件，扫描和 DAG 构建可能变慢。
    *   **对策**: 流式处理 + 延迟加载。

## 6. 优先级与排期建议
此功能涉及执行引擎的深度改造，建议排在 **P2.5 (脚本之后，文档之前)**。
*   **Sprint 4**: 实现 Sequential Mode + 依赖解析 (MVP)。
*   **Sprint 5**: 实现 Parallel Mode + Reporting。
