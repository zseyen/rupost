# 2026-06-16 启发式自适应请求拆分 (Heuristic Auto-Split) PRD 与设计方案

本设计方案旨在解决在同一个 Markdown 的 HTTP 代码块中写入多个用例时，由于缺失 `###` 分隔符而导致的解析错乱（元数据被覆盖、Body 误合并及断言被累加至单个请求）的问题。我们决定采用**方案一（自适应启发式块拆分）**进行底层引擎重构。

---

## 🎯 业务背景与痛点 (Context & Problem)

*   **背景**：Rupost 倡导“文档即测试”，允许在 Markdown 文档的 http 代码块中快速堆叠多个 API 测试用例。
*   **痛点**：当前解析器强依赖 `###` 分隔符进行物理块划分。但在真实开发或文档书写中，很多开发者（包括智能体）会习惯性地连续编写多个带有 `@name`、`@test` 或 `GET/POST` 的用例，而写漏了 `###`。
*   **后果**：这导致后续的请求头和 Body 连同请求行全部被误当成第一个请求的 Body 发送，触发 JSON 格式解析失败或断言拦截，产生极其隐晦且难于排查的 Bug。

---

## 🚀 目标与验收条件 (Goals & Acceptance Criteria)

### 核心目标 (Goals)
1.  **无感智能分割**：解析器应能够自适应识别代码块中的用例边界。即使完全没有写 `###`，也能够自动、正确地将它们拆分成独立的 `ParsedRequest`。
2.  **向后兼容**：保留对原有标准 `###` 分隔符的支持。
3.  **零功能破坏**：不破坏现有的变量替换、断言校验、Mock 变体解析以及级联拓扑运行功能。

### 验收用例 (Acceptance Criteria)
*   **Given**: 一个 HTTP 代码块包含多个 `@name` 和请求行，且中间没有 `###`：
    ```http
    @name test-1
    GET /api/v1/users
    
    @name test-2
    POST /api/v1/users
    {}
    ```
*   **When**: 运行解析器解析该文件。
*   **Then**: 解析器应输出 2 个独立的 `ParsedRequest`，分别名为 `test-1` 和 `test-2`。没有发生请求行被合并为 Body 的现象。

---

## 🛠️ 技术设计与实现细节

### 1. 启发式切分核心算法
我们将升级 [src/parser/http_file.rs](file:///Users/zsyzzx/project/rust/rupost/src/parser/http_file.rs) 中的 `split_by_separator` 切割器。在按行扫描时，遇到以下四类特征行中的任意一类，且 `current_block` 非空时，即判定为新请求的分隔线：

*   **元数据开始**：以 `@name ` 或 `@test` 开头的行。
*   **合法请求行**：以 `GET `、`POST `、`PUT `、`DELETE `、`PATCH `、`HEAD `、`OPTIONS ` 开头，且后续跟有 URL 的行。
*   **显式分隔符**：以 `###` 开头的行。

#### 核心解析算法伪代码：
```rust
fn split_by_separator(content: &str) -> Vec<(String, usize)> {
    let mut blocks = Vec::new();
    let mut current_block = String::new();
    let mut block_start_line = 1;

    for (current_line, line) in (1..).zip(content.lines()) {
        let trimmed = line.trim();
        
        // 判定该行是否是新请求的开始特征行
        let is_new_request_indicator = trimmed.starts_with("###")
            || trimmed.starts_with("@name")
            || trimmed.starts_with("@test")
            || is_valid_request_line(trimmed);

        // 如果检测到新请求指标，且当前块非空，则进行自适应切分
        if is_new_request_indicator && !current_block.trim().is_empty() {
            // 如果是以 ### 开头的分割行，当前块直接保存，新块行号为下一行
            // 如果是以 @name/@test 等元数据开头，它们属于新块的头部，所以应将其作为新块的起始行
            blocks.push((current_block.clone(), block_start_line));
            current_block.clear();
            block_start_line = current_line;
        }

        // 注意：如果是 ### 分隔符行，本身不需要存入新块的 content 中
        if !trimmed.starts_with("###") {
            current_block.push_str(line);
            current_block.push('\n');
        }
    }

    if !current_block.trim().is_empty() {
        blocks.push((current_block, block_start_line));
    }

    blocks
}
```

### 2. 请求行判定规则
我们需要实现 `is_valid_request_line` 判定：
```rust
fn is_valid_request_line(line: &str) -> bool {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.is_empty() {
        return false;
    }
    let method = parts[0].to_uppercase();
    let valid_methods = ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"];
    
    // 如果首个单词是合法的 HTTP Method，且该行有后续的 URL 部分
    valid_methods.contains(&method.as_str()) && parts.len() >= 2
}
```

---

## 🔮 长期重构与解析器改造建议 (Long-term Refactoring Proposals)

为了进一步提升 `rupost` 解析器的大规模工业级稳定性，本 PRD 同步归档了如下三项中长期架构改造规划，可在完成启发式自适应块拆分后逐步实施：

### 方案 A：统一 JSONPath 评估引擎
*   **痛点**：Mock 条件分支匹配使用的 `resolve_jsonpath` 无法支持数组索引定位（例如 `$.0` 或 `$[0]`），而测试断言端的解析器能够支持，这导致了断言与 Mock 语义的割裂。
*   **改造建议**：废除 Mock 端简易的局部实现，将 JSONPath 解析功能抽象为 Core Domain 的 `JsonPathResolver`。在分析字段时，自动判断 `serde_json::Value` 类型，若当前节点是 Array 且 key 分段为整型数字，自动解析为 `usize` 数组索引，实现全平台断言与 Mock 解析逻辑的 100% 对齐。

### 方案 B：行状态机严格报错与 Linter 诊断（软警告）
*   **痛点**：对于格式非法的报文行（如拼写错误的 Header `Content-Type application/json`），目前的行状态机会静默忽略，增加排查难度。
*   **改造建议**：
    *   在 `ParsedFile` 引入 `diagnostics: Vec<Diagnostic>` 结构。
    *   在解析时，不以致命异常中断编译（对用户友好），而是将解析失败行转化为软警告，在 Mock 启动或测试跑完时于终端下方亮黄输出：
      `[Warning] path/to/file.md:L78 - Line 'Content-Type...' could not be parsed as a header. Did you forget a colon?`

### 方案 C：扩展并标准化 `None`（空值）语义
*   **痛点**：目前只强硬匹配了不带引号的 `"None"` 字面量，不支持 `null`、`undefined` 或带引号的字符串区分。
*   **改造建议**：将空值拦截匹配泛化，在编译时将 `None`、`null`、`NULL`、`nil`、`undefined`（未被双引号包裹时）统一映射为 `CompareOp::Exists` 且值为 `"None"` 的缺失校验逻辑；而带双引号的 `"None"` 则恢复为普通字面量字符串比对。

---

## 📈 回归测试与进度说明

### 当前开发进度
*   **Stage 0 & 1 (分析与架构设计)**: 已完成方案一（自适应拆分）与长期重构方案的设计归档，正在制定实施计划。 [Pending User Approval]
*   **Stage 2 & 3 (具体实现与测试)**: 待用户确认后开始实施。

### 自动化验证计划
在完成 `split_by_separator` 的重写后，我们需要：
1. 编写专门的单元测试 `test_parse_multiple_requests_without_separator`，验证不带 `###` 的多请求解析成功。
2. 运行 `cargo test` 运行全量单元和集成测试。
3. 执行 `./rupost test examples/iteration_scenarios/migration_test.md` 验证示例契约全部通过。
