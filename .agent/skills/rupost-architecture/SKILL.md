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
