# Rupost 开发进度与状态 Checkpoint

## 当前状态 (Current State)
1. **URL 智能拼接与快捷缩写 (URL Resolution & Shortcuts)**:
   - 全面支持 CLI 智能协议补全 (例如域名、`localhost:port` 自动补全默认协议) 与冒号本地快捷键。
   - 实现测试文件中的冒号局部快捷键自适应转换与层级分层拼装。
   - 彻底将 URL 拼接与 WS、SSE 分流前置对齐，保持了 clean architecture。
   - 移除 `VariableResolver` 的相对路径拼接耦合，保持其职责单一。
   - 单元测试与集成测试通过率达到 100%。

2. **WS 领域层重构 (Clean Architecture)**:
   - 已将帧匹配职责解耦并迁移至 `src/ws/matcher.rs` 的 `WsConditionMatcher`。
   - 单元测试与集成测试通过率达到 100%。

3. **声明式网络诊断与断言拓展 (Declarative Diagnostics & Assertion Metrics)**:
   - 支持通过在测试文档中添加 `# @diagnose` 注解，按需触发网络诊断。
   - 提取了共享的无状态 `connect_tcp_with_timeout` Socket 测量组件，消除了 `timing` 与 `diagnose` 模块的重复代码。
   - 引入 `timing.ttfb/dns/tcp/tls` 和 `cert.days_remaining/issuer/subject` 的断言解析与提取，允许直接在用例中编写契约断言。
   - 对所有网络探测阶段（DNS、TCP、TLS、TTFB 响应）应用了 `tokio::time::timeout` 超时控制，消除 CI/CD 卡死隐患。
   - 新增了 `rupost diagnose --report json` 结构化导出支持，便于自动化集成。
   - 编写了 [assertion_diagnose.http](file:///Users/zsyzzx/project/rust/rupost/examples/diagnose/assertion_diagnose.http) 冒烟测试并完美通过；全量 166 个测试用例回归通过率 100%。

4. **版本库提交管理 (JJ Commits)**:
   - 使用 `jj` 小步提交，最新提交说明：`feat: complete declarative network diagnostics with assertions, unified timings, and JSON CLI report`。

## 下一步工作 (Next Steps)
- 启动 Sprint 6：导出可视化 HTML 报告与模板表现层。
- 进一步优化并发测试调度的实时 TUI 表现。


