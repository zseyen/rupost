# RuPost Cookie 功能与测试技术规约 (Cookie Test & Architecture Specification)

本文件详细记录了 RuPost 项目中 Cookie 功能的架构设计、安全保障机制以及配套的集成测试体系，供开发和架构评审参考。

---

## 1. 核心架构与功能设计

RuPost 遵循 **Clean Architecture** 及 **Micro-Kernel + Middleware** 设计模式。Cookie 功能被封装为一个独立的中间件插件 [CookieMiddleware](file:///Users/zsyzzx/project/rust/rupost/src/middleware/cookie.rs)，其生命周期拦截在请求执行完毕后。

### 1.1 拦截与数据流向
1. **注入阶段**：
   在构建 HTTP 客户端 [Client](file:///Users/zsyzzx/project/rust/rupost/src/http/client.rs) 时，若启用了 Cookie 支持，会将底层的内存 Cookie 库 `Arc<CookieStoreMutex>` 注册为 `reqwest::Client` 的 `cookie_provider`。
   * **特点**：基于 RFC 6265，`reqwest` 将在发生 HTTP 请求发送前**自动**从 CookieStore 中筛选匹配 Domain 和 Path 的 Cookie 并注入到 Request Header 中。因此，`CookieMiddleware::before_request` 不需要做额外的 Header 拼接工作。
2. **提取与持久化阶段**：
   在请求执行成功并返回 [Response](file:///Users/zsyzzx/project/rust/rupost/src/http/response.rs) 后，执行器会显式调用 `CookieMiddleware::after_response` 中间件拦截器：
   * **自动捕获**：`reqwest` 的内置 Provider 在收到响应后自动解析 `Set-Cookie` 字段并归档到内存 CookieStore 中。
   * **自动持久化**：如果是 `AutoPersist` 模式，中间件会立即调用 `.save()` 将内存中的 CookieStore 序列化并安全写入到本地文件中。

### 1.2 三种运行模式 (CookieMode)
*   **`AutoPersist` (自动持久化)**：从特定的 `cookies.json` 文件路径加载已保存的 Cookie，每次请求响应结束或中间件被 Drop 析构时，自动刷新并回写到磁盘文件中。
*   **`Ephemeral` (会话级临时)**：只在内存中创建临时的 CookieStore，运行结束后随进程退出而销毁，适用于单次独立无污染测试。
*   **`Disabled` (禁用模式)**：完全不注入任何 CookieStore，客户端不接收也不发送 Cookie，适合绝对无状态请求。

---

## 2. 并发安全与异常降级机制

### 2.1 多进程读写冲突保护 (Advisory File Locking)
为了保证多个终端同时运行 RuPost 时不会由于写入冲突（如截断一半）导致 `cookies.json` 文件损坏，我们采用 `fs2` crate 实现了严密的**文件咨询锁**策略：
*   **读取锁定 (`lock_shared`)**：在 `load_or_create` 加载文件时加**共享读锁**。支持多个进程同时并发读取该文件。但如果某个进程正在写入（持有独占写锁），则当前读取会阻塞等待其写入完毕，防止读取到写到一半的非法/破损数据。
*   **写入锁定 (`lock_exclusive`)**：在 `save` 持久化时加**排他写锁**。在写入期间阻止任何其他进程的读、写操作，保证数据的绝对原子性和一致性。

### 2.2 异常 JSON 解析降级 (Graceful Fallback)
如果由于外界干扰或手动篡改导致本地的 `cookies.json` 变成非法的损坏 JSON 数据，`load_or_create` 不会采取崩溃抛错退出导致 executor 初始化失败的策略，而是：
1. 捕获解析 Error。
2. 使用 `tracing::warn!` 记录详细日志（以便用户知悉）。
3. 降级为全新的空 `CookieStore::default()` 恢复启动。
4. 保证测试流程的继续执行。

---

## 3. 集成测试套件说明

目前在 [cookie_test.rs](file:///Users/zsyzzx/project/rust/rupost/tests/cookie_test.rs) 中实现并运行了 9 个集成测试用例，覆盖了从底层协议细节到并发边缘的全部场景：

### 3.1 协议及核心能力测试
1.  **`test_cookie_sending`**：
    *   **测试目的**：验证当 CookieStore 中已存有特定 Cookie 时，客户端能够自动筛选并在请求中带上 `Cookie` 头部。
2.  **`test_set_cookie_receiving`**：
    *   **测试目的**：验证服务器下发 `Set-Cookie` 时，客户端能够成功拦截并在响应后将其保存至 CookieStore 内存中。
3.  **`test_cookie_domain_and_path_matching`**：
    *   **测试目的**：验证 RFC 6265 作用域匹配。
    *   **测试逻辑**：注入域名为 localhost 的 Cookie 和限定 Path 为 `/sub` 的 Cookie。访问 `/other` 时，验证仅发送了域名 Cookie；访问 `/sub/page` 时验证同时发送了两个 Cookie。
4.  **`test_cookie_expiration`**：
    *   **测试目的**：验证已过期的 Cookie 不会被发送。
    *   **测试逻辑**：注册一个 `Max-Age` 为 1 秒的短期 Cookie，`tokio::time::sleep` 1.5 秒后发起请求，验证服务端仅能收到另一个长效的 Cookie。

### 3.2 存储与运行模式测试
5.  **`test_cookie_persistence`**：
    *   **测试目的**：验证 `AutoPersist` 模式下，Cookie 能够成功写入文件，并由另一个全新实例顺利加载出来。
6.  **`test_session_and_persistent_cookies`**：
    *   **测试目的**：验证 Session 级 Cookie (无到期时间) 和 Persistent 级 Cookie 在持久化时都能被成功存入 JSON 文件，并重新读出还原。
7.  **`test_cookie_disabled`**：
    *   **测试目的**：验证禁用 Cookie 时，两次请求间不会发生任何状态保持。

### 3.3 并发与容错鲁棒性测试
8.  **`test_cookie_file_corruption_recovery`**：
    *   **测试目的**：验证解析损坏的 JSON 文件时，系统能记录 warn 并降级初始化空 store 恢复可用性。
9.  **`test_concurrent_cookie_access`**：
    *   **测试目的**：模拟 10 个线程并发对同一个 `cookies.json` 进行高频 `load` -> `insert` -> `after_response` 保存。利用共享读锁与独占写锁机制，保证整个过程数据不损坏且不发生 Panic。
