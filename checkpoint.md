# Checkpoint - Rupost WebSocket MVP & Architectural Evolution

## 当前状态
- **核心重构与功能接入**：
  - 成功接入了纯 WebSocket 协议的 MVP 自动化测试与调试能力。
  - 在 `src/ws/client.rs` 中设计并实现了基于 **Actor 模式的多路广播 `WsClient`** 自动维护长连接。
  - 开发了 `WsActionParser` 动作解析器与 `WsRunner` 动作执行引擎，并将 `src/runner/executor.rs` 进行路由重定向分流，完美契合开闭原则 (OCP)。
  - 支持并验证了二进制解码匹配，修复了二进制帧在 EXPECT 条件匹配判定前未提前解码的缺陷，补充了 `test_websocket_e2e_msgpack_binary` 测试，验证了 MsgPack 二进制消息解码匹配。
  - 新增了 `examples/websocket.http` 与 `examples/websocket.md` 示例文件。
- **Stage 1: 统一时间解析与诊断工具升级**：
  - 升级并重用了全局 `parse_duration` 逻辑，兼容可选的 `=` 前缀，并统一了 `WsActionParser` 的局部 `@timeout` 行。
  - 重构了 `src/http/diagnose.rs`，不仅支持普通的 HTTP/HTTPS 诊断，更支持对 `ws/wss` 的物理连通性和 101 WebSocket Upgrade 协议升级进行双重协商诊断，高亮提示 Nginx Upgrade 转发头配置故障。
- **Stage 2: 握手安全中间件链与 CookieStore 会话共享**：
  - 重构了路由分流层，将 `RoutingMiddleware` 中间件链应用于 WebSocket 握手请求。确保 Host 重写、Bearer Token 注入、密钥扫描等安全合规策略对 WebSocket 物理握手 100% 生效。
  - 打通了 Cookie 共享，在握手建连前自动从全局 `CookieStore` 抽取同 Host 的 Cookie 注入物理 Upgrade Request 中，实现了 HTTP 登录与 WebSocket 长连接会话状态的平滑承接。
- **质量保证与版本控制**：
  - 新增了 `test_diagnose_websocket_upgrade_success`、`test_websocket_middleware_and_routing` 以及 `test_websocket_cookie_inheritance` 等集成测试，完美覆盖了 101 升级、中间件重写与 Cookie 会话继承。
  - 所有 206 个单元与集成测试全部绿灯通过。
  - 使用 `jj` 进行了原子化版本提交（Stage 1: `klqtunvw 023f0695`，Stage 2: `oztykwsr 9ad6009b`）。

## 下一步工作
- **Stage 3: FrameMatcher 抽象与 JSONPath 高效预编译集成**：
  - 定义 `FrameMatcher` 接口，解耦具体判定逻辑。
  - 实现 `JsonPathMatcher` 并集成 `JsonPathResolver`，在解析阶段对表达式做预编译以在高频下获得极佳性能。
- **Stage 4: 步骤级局部断言与变量捕获**：
  - 扩展 `WsAction::Expect` 支持局部断言与捕获链。
  - 匹配成功的第一时间执行局部 `@assert` 和 `@capture`。
- **Stage 5: WsSession 代理、自动重连与发送缓冲自愈**：
  - 编写 `WsSession` 与带发送队列 `pending_send_queue` 的 `SessionManager`，重连成功顺序排干。
- **Stage 6: 内存滑动限制环形缓冲区**：
  - 实现 `BoundedFrameBuffer` 限制缓存为 1000 帧保护内存。
