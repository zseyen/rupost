# Checkpoint - Rupost WebSocket MVP & Step-Level Assertion Pipeline

## 当前状态
- **核心重构与功能接入**：
  - 成功接入了纯 WebSocket 协议的 MVP 自动化测试与调试能力。
  - 在 `src/ws/client.rs` 中设计并实现了基于 **Actor 模式的多路广播 `WsClient`** 自动维护长连接。
  - 开发了 `WsActionParser` 动作解析器与 `WsRunner` 动作执行引擎，并将 `src/runner/executor.rs` 进行路由重定向分流，完美契合开闭原则 (OCP)。
  - 支持并验证了二进制解码匹配，修复了二进制帧在 EXPECT 条件匹配判定前未提前解码的缺陷，补充了 `test_websocket_e2e_msgpack_binary` 测试，验证了 MsgPack 二进制消息解码匹配。
  - 新增了 `examples/websocket.http` 与 `examples/websocket.md` 示例文件。
- **Stage 1 & 2: 统一时间解析、101诊断与中间件 Cookie 继承**：
  - 升级并重用了全局 `parse_duration` 逻辑，兼容可选的 `=` 前缀，并统一了 `WsActionParser` 的局部 `@timeout` 行。
  - 重构了 `src/http/diagnose.rs`，不仅支持普通的 HTTP/HTTPS 诊断，更支持对 `ws/wss` 的物理连通性和 101 WebSocket Upgrade 协议升级进行双重协商诊断，高亮提示 Nginx Upgrade 转发头配置故障。
  - 将 `TestExecutor` 的中间件流水线应用 to WebSocket 升级握手请求上，保障 Host 重写、密钥审计和 Cookie 共享 100% 成功。
- **Stage 3 & 4: FrameMatcher 预编译与步骤级局部捕获/断言**：
  - 在 `src/ws/matcher.rs` 中定义了统一的 `FrameMatcher` 接口。实现了 `JsonPathMatcher` 与普通包含匹配 `TextContainsMatcher`，并支持预编译 JSONPath Segments，将计算性能优化至最高。
  - 扩展了 `WsAction::Expect` 挂载局部 `assertions` 与 `captures`。在 `ws_runner.rs` 命中帧的第一时间直接运行局部捕获写入 Context 和断言，让变量在动作步骤之间**级联流动**，且局部断言不受高频心跳广播帧的误伤。
- **Stage 5 & 6: WsSession 逻辑会话、重连自愈与滑动环形历史缓冲区**：
  - 在 `src/ws/session.rs` 中构建了逻辑会话层 `WsSession` 与后台守护 `SessionManager`，无缝替换了原先的 `WsClient`，在遇到物理断网时采用指数退避机制自动重连（最大重试 5 次，总计约 30 秒）。
  - 在重连期间，逻辑发帧请求将暂存于限额为 100 帧的发送缓冲区 `pending_send_queue` 中；重连恢复后执行强时序 Flush 补发，保障消息数据零丢失。
  - 实现了 `BoundedFrameBuffer`（容量上限 1000 帧），融入会话收发帧历史记录中，避免高频广播行情调试时的内存膨胀。
- **质量保证与版本控制**：
  - 新增了 `test_websocket_reconnect_and_flush_self_healing` 等高价值 E2E 集成测试，验证了断线自愈、补发、环形历史区及步骤级变量 Cascade 流转。
  - 进行了 WebSocket Code Review，全面修复了 `JsonPathMatcher` 操作符匹配反转、重连队列时序漏洞、Action 执行计数不准等边界情况。
  - 在 `matcher.rs`、`session.rs` 和 `action_parser.rs` 中为上述核心逻辑补充了全方位的单元测试，并在 `websocket_integration_test.rs` 中新增了 `test_websocket_e2e_jsonpath_operators` 集成测试。
  - 全量 212 个集成与单元测试 100% 绿灯跑通。
  - 使用 `jj` 进行了原子化版本提交。

## 下一步工作
- **Sprint 6: HTML 报告与高级表现层 (下阶段启动)**：
  - 导出可视化 HTML 报告与模板表现层，提升批量测试执行结果的直观程度。
- **TUI 交互面板的异步实现 (下阶段启动)**：
  - 待本阶段核心引擎 100% 健壮交付后，下阶段落地基于单向数据流与 Crossterm/Ratatui 事件循环的多分栏调试面板。
