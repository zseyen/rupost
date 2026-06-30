---
name: rupost-architecture
description: "Guidelines for Rupost's Micro-Kernel + Middleware architecture, Clean Architecture enforcement, and core request/response data flow."
---

# Rupost Architecture Mastery

## Core Philosophy: "Kernel + Middleware + UI"

Rupost follows a micro-kernel pattern where the core logic is kept minimal, and all extended features are implemented as Middlewares.

### 1. The Middleware Chain
Any logic that intercepts or modifies a request/response (Cookies, Auth, Scripts, Logging) MUST implement the `Middleware` trait:

```rust
pub trait Middleware: Send + Sync {
    /// Executed before the HTTP request is sent.
    async fn before_request(&self, req: &mut Request) -> Result<()>;
    
    /// Executed after the HTTP response is received.
    async fn after_response(&self, resp: &Response) -> Result<()>;
}
```

- **Injections**: Use `before_request` to inject Headers or modify URLs.
- **Extractions**: Use `after_response` to capture Cookies or variables.

### 2. Data Transformation Flow
Respect the strict separation between the parsed representation and the executable representation:

1. **`ParsedRequest`**: Raw data from `.http` or `.md` files (found in `src/parser`).
2. **`Request`**: The actual executable request used by the HTTP engine (found in `src/http`).

Transition logic MUST use `TryFrom<ParsedRequest> for Request` to ensure early validation.

### 3. Clean Architecture Layers
Always maintain these boundaries:
- **`src/core`**: Orchestration logic (The Kernel).
- **`src/http`**: Low-level networking (wraps `reqwest`).
- **`src/parser`**: File format grammars.
- **`src/middleware`**: Feature extensions.
- **`src/ui`**: TUI/CLI presentation.

## Concurrency & Safety
- **Async Strategy**: Use `Tokio`. Avoid blocking the main event loop.
- **File Access**: When multiple processes might access common files (like `cookies.json`), use `fs2` for advisory locking.
- **Shared State**: Prefer `Arc<T>` with fine-grained Mutexes or RwLocks only when necessary.

## Implementation Rules
1. **No Logic in Main**: `main.rs` should only handle CLI dispatching and error reporting.
2. **New Features = New Middleware**: If adding a feature like "Auto-Retry", create a new middleware instead of modifying the `HttpEngine`.
3. **Implicit Over Explicit**: Prefer intelligent defaults (e.g., auto-detecting JSON body) but allow explicit overrides via Metadata (`@metadata`).

## Network Diagnostics & Assertion Metrics Flow

RuPost 提供了一套三维一体的底层网络与安全诊断系统。为确保高频测试下的性能与异常发生时的易用性，其数据流与触发链路必须遵循以下规范：

### 1. 三维触发与判定链路 (Trigger Mechanism)
- **声明式优先 (`# @diagnose`)**：用例文件若显式打标，必须在请求返回后、断言执行前立刻运行网络诊断，以便将 `DiagnosticsReport` 挂载入 `Response`，支撑 `timing.*` 与 `cert.*` 指标断言提取。
- **全局覆盖 (`--debug`)**：属于全局命令行选项。若启用，则无视元数据，强制在断言前执行网络诊断。
- **后置补测 (`--debug-on-failure`)**：针对高并发与 CI 巡检优化。常规状态下零开销；一旦用例被判定为失败（`!success`），若此前没有执行过诊断，则在此处进行异步补测并保存至 `TestResult`。
- **异常分支诊断**：若在物理链路层遭遇断开或拒绝（`Err(e)` 分支），当开启诊断时，直接运行诊断挂载至 `TestResult::diagnose_report`。

### 2. 零侵入向前兼容设计 (Backward Compatibility)
- `TestResult` 中新的 `diagnose_report` 必须作为 `Option` 承载。
- 测试结果报告器（TUI 渲染层）应优先展示 `diagnose_report` 瀑布图；若为 `None`，则必须无缝回退渲染原有的 `timing: RequestTiming` 数据，确保旧用例与旧测试百分之百不被影响。

