# Checkpoint - Rupost WebSocket MVP Support

## 当前状态
- **核心重构与功能接入**：
  - 成功接入了纯 WebSocket 协议的 MVP 自动化测试与调试能力。
  - 在 `src/ws/client.rs` 中设计并实现了基于 **Actor 模式的多路广播 `WsClient`** 以及双层后台 Worker 心跳保活协程，自动维护长连接的 Ping/Pong 和业务层数据交换。
  - 开发了 `WsActionParser` 流式动作解析器，支持在 `.http` 或 `.md` 中以 `@websocket` 指令声明长连接交互流，解析 `SEND` / `EXPECT` 的动作逻辑。
  - 编写了 `WsRunner` 动作执行引擎，并将 `src/runner/executor.rs` 进行路由重定向分流，完美契合开闭原则 (OCP)，不侵入已有的 HTTP 与 SSE 机制。
- **质量保证与测试套件**：
  - 补充了 `ws::client` 握手与多通道广播单元测试、`ws::action_parser` 动作段词法测试。
  - 编写了 `tests/websocket_integration_test.rs` 真实网络下的 E2E 集成测试，验证了高频心跳广播干扰下的 EXPECT 软匹配以及全局变量的 `@capture` 提取与 `@assert` 契约验证。
  - 所有新增的测试套件及原有全量用例在本地离线模式 (`cargo test --offline`) 下全部绿色跑通。
  - 优雅解决了 WebSocket 流式剧本 body 中局部 `@timeout` 与 HTTP 全局元数据解析器的冲突（当处于 WebSocket 用例且跨过空行后，自适应豁免 HTTP 级元数据提取，保留至 body 中）。
- **版本控制**：
  - 使用 `jj` 进行了原子级阶段提交，当前工作拷贝包含完备的 WebSocket 调试与测试 MVP 全量逻辑。

## 下一步工作
- **流量快照录制与双角色回放**：
  - 实现基于 `.jsonl` 的帧录制器。
  - 实现 `rupost ws replay`，支持参数化 Client 相对时延重放与 Server 模式本地脱机灌入。
- **TUI 交互调试面板**：
  - 基于 `ratatui` 开发带文档大纲导航、标签分流、内置草稿编辑和故障模拟的 WS 手动联调终端。
