# Rupost 开发进度与状态 Checkpoint

## 当前状态 (Current State)

1. **HTTP 请求/响应基础快照录制与原样重放 (MVP 闭环)**:
   - **快照主动录制**：在 `rupost test` 命令中扩展了可选参数 `--save-snapshot <file>`，测试运行完毕后可一键将请求/响应数据序列化为结构化 JSON 快照文件。
   - **原样重放与 Host 覆写**：新增了 `rupost replay <file> [--target <url>]` 命令，加载快照并发送实际网络请求，重放时支持通过 `--target` 参数自动将请求重写到指定的内网测试网关（协议/Host/Port 覆盖），对状态码进行自动化比对和终端高亮输出。
   - **非侵入式架构**：重载 `TestResult` 增加 `request: Option<RequestSnapshot>` 数据流，并在 `execute_one` 中非侵入式地捕获源请求快照，完全保障了原本核心执行流和其它测试用例的高稳定性。
   - **自动化集成测试**：编写了专有集成用例 [tests/snapshot_replay_test.rs](file:///Users/zsyzzx/project/rust/rupost/tests/snapshot_replay_test.rs)，基于本地 Wiremock 服务器测试全过程闭环并 100% 通过。
   - **版本化提交 (JJ)**：已使用 `jj` 工具对当前所有代码修改进行安全提交，版本信息为：`feat: implement HTTP snapshot recording and replaying MVP`。

2. **声明式网络诊断与断言拓展 (Declarative Diagnostics & Assertion Metrics)**:
   - 全面支持通过添加 `# @diagnose` 注解触发网络诊断时延瀑布图和 X.509 证书深度分析。
   - 支持对 `timing.ttfb/dns/tcp/tls` 和 `cert.days_remaining` 的直接声明式契约断言，支持 `rupost diagnose --report json` 结构化导出。

3. **Sprint 5: WebSocket 协议测试与调试**:
   - 支持在 `.http` 或 `.md` 中以 `@websocket` 声明长连接，解析运行 `SEND` / `EXPECT` 等流式剧本动作，支持 MsgPack 二进制自适应解码和滑动内存历史帧环形缓冲区控制。

---

## 下一步工作规划 (Next Steps)

按照新制定的 [后续功能路线图 (Roadmap)](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-07-01-snapshot-replay-roadmap.md)，我们将推进以下扩展功能：

- **敏感数据就地脱敏 (Secrets Masker)**：在快照落盘前，对 Authorization、Cookie 字段及 Body 敏感正则词执行就地掩码打码替换。
- **智能 JSON Diff 引擎**：重构重放对比逻辑，对 JSON Body 默认解析并智能过滤掉如 `timestamp`、`request_id` 等易频繁抖动的动态字段，避免误报差异。
- **超时与断网事故现场捕获**：网络完全不通时自动在快照中生成 `status = 599` 的虚拟响应，完整记录连接错误日志。
- **事后从历史数据导出快照**：重构 `HistoryEntry` 携带 Body，并开发 `rupost history export --last <N>` 的补录导出。
