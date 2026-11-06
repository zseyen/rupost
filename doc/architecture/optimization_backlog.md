# 优化点记录 (Optimization Log)

## 1. 变量替换的健壮性检查 (Robustness)

**当前问题** (Current Issue)：
目前的 `VariableResolver::substitute` 逻辑是：如果在上下文中找不到变量，就保留原样（例如 `{{missing_var}}`）。
如果由于由于请求顺序不对导致变量尚未被捕获，或者用户拼写错误，这会导致：
1.  请求带着未替换的 `{{...}}` 字符串发送给服务器。
2.  服务器返回 400 Bad Request 或 401 Unauthorized。
3.  用户难以调试，因为没有明确的报错提示“变量未定义”。

**优化方案** (Proposed Solution)：
在 `TestExecutor::execute_one` 发送请求前，增加一个检查步骤：
1.  检查替换后的 URL、Headers、Body 中是否仍然包含 `{{...}}` 模式。
2.  如果存在，打印 `WARN` 日志，提示用户：
    *   变量可能拼写错误。
    *   变量可能尚未被捕获（检查请求顺序）。
    *   或者这确实是一个字面量（虽然少见）。

**实现思路**：
```rust
// 伪代码
if text.contains("{{") && text.contains("}}") {
    warn!("Potential unresolved variable detected in request: {}", text);
}
```

## 2. 错误处理的进一步统一
虽然 `capture.rs` 已经使用了 `RupostError`，但项目中可能还有其他地方使用了 `unwrap()` 或 `anyhow`。建议全项目统一使用 `RupostError`。

## 3. P3 预留功能的完善
- **Trace ID 自动捕获** (`CaptureSource::TraceHeader`)：自动识别调用链追踪 ID（如 AWS X-Ray, Zipkin）。
- **Cookie 保持** (`CaptureSource::Cookie`)：实现 Cookie Jar 机制，自动处理 Session。

## 4. 错误处理的最佳实践 (Code Quality)
**背景**：
我们在 `src/variable/capture.rs` 中重构了错误处理，从 `anyhow` 迁移到了 `RupostError` (thiserror)。

**收益**：
1.  **库层 (Library) vs 应用层 (App)**：核心逻辑层应提供强类型的错误定义 (`enum`)，而非模糊的动态错误 (`anyhow`)。
2.  **可匹配性**：调用方可以针对特定的错误类型（如 `ParseError`）做降级处理，而不是只能打印字符串。
3.  **架构清晰**：统一管理错误码和消息格式，符合 Clean Architecture。

**后续行动**：
- 审查 `src/parser` 和 `src/runner` 模块，确保所有核心逻辑都尽量避免直接使用 `anyhow!`，而是通过 `RupostError` 抛出。
