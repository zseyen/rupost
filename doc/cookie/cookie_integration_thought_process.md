# Cookie 接入思路说明 (Cookie Integration Thought Process)

## 需求分析

在 `RuPost` 中，我们希望扩展现有的文件测试 (`.http` 文件) 以及命令行接口 (`CLI`)，使之支持自动的 Cookie 管理。根据之前讨论和本项目的目标：
1. **默认使用内存（Ephemeral）Cookie**：在没有明确要求禁用或持久化的情况下，同一个测试文件中相邻的请求、以及单个命令执行时的生命周期内，应该默认可以携带自动保存和发送的 Cookie。
2. **支持禁用 Cookie**：对于一些严格要求无状态测试的场景，可以通过 `--no-cookies` 禁用该功能。
3. **支持 Cookie 本地持久化**：为了能在不同的命令行调用或测试文件之间共享认证状态，应该支持通过 `--cookie-file <path>` 将 Cookie 存储到特定文件中。

## 当前架构能力

经过对源码的初步探查：
* **`cookie.rs`**：已经实现了 `CookieMiddleware`，提供三种模式 (`AutoPersist`, `Ephemeral`, `Disabled`)，底层利用 `reqwest_cookie_store` 处理实际的保存逻辑与自动携带机制。
* **`client.rs`**：核心的 HTTP Client `Client::with_cookie_store` 可以接受一个配置好的 `CookieStoreMutex` 进而开启 Cookie 管理。
* **`executor.rs`**：`TestExecutor::with_cookies` (持久化) 和 `TestExecutor::with_ephemeral_cookies` (内存) 也已经就绪，但并未在这两个主执行入口被使用。
* **`main.rs` & `cli.rs`**：目前测试执行 (`run_test`) 和命令行请求执行 (`CliRunner::run`) 均使用了无 Cookie 版本的 `TestExecutor::new()`。

## 具体落地思路

1. **CLI 参数层修改**
   为了给用户提供选项，在 `struct Commands::Test` 中增加：
   * `--no-cookies`(`bool`)：标记是否禁用 Cookie。
   * `--cookie-file`(`Option<String>`)：标记使用的 Cookie 持久化地址。

2. **核心业务组装层 (`run_test`)**
   修改 `main.rs` 的 `run_test`，获取这两个参数并在实例化 `TestExecutor` 时做出判断：
   * 优先判断是否 `--no-cookies`。如果在测试时确实不需要，则使用纯净的 `new()`。
   * 如果指定了 `--cookie-file`，则使用 `with_cookies()` 确保会落盘和读取。
   * 如果用户什么都没输入，则默认使用 `with_ephemeral_cookies()`，使得当前文件的多次 Request 自动共享 Cookie。

3. **命令行快捷调用的修改 (`CliRunner`)**
   除了文件执行，普通的 `rupost GET http://example.com` 也可以自动具备 ephemeral cookie 功能，在重定向跟踪中这尤为重要。将 `CliRunner::new` 实例化时调整为 `with_ephemeral_cookies()`。

4. **集成测试验证 (`examples/cookie_demo.http`)**
   为验证以上功能，创建一个 `.http` 文件。
   * 第一个请求访问 `httpbin.org/cookies/set?k=v`。
   * 第二个请求访问 `httpbin.org/cookies`。
   * 利用断言系统 (`@assert result.body contains "v"`) 验证 Cookie 确已被管理。

此思路保持了 `Clean Architecture` 的分层职责清晰，修改范围极小且能达到预期的功能集成。
