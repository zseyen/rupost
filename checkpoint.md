# RuPost 质量加固战役 Checkpoint 归档（2026-10）

## 当前状态总结 (Current Status)

依据 [质量加固执行方案](doc/plans/2026-10-18-quality-hardening-execution-plan.md)（v3），Phase 0–4 全部完成，**里程碑 C 达成**：方案内全部 25 项审查发现闭环。全程 `jj` 原子提交（修复与对抗性回归测试同提交）。

### Phase 0–1：发布阻塞与 panic 清零
- MSRV 修正为 1.89，完成 fs2 → std 文件锁迁移，clippy 清零
- 三个实测 panic 修复：非标准状态码（600–999）、单引号断言值切片、非法 header 名——全部改为产生失败的测试结果而非崩溃

### Phase 2：核心正确性
- **共享最左运算符扫描器**（`assertion::find_operator`）：引号感知 + 同位置最长优先 + `contains` 词边界；`@assert` 与 WS `EXPECT` 两处同源缺陷一次修复，WS 侧补齐 `>= <= > <`
- **`@timeout` 真正生效**：reqwest per-request 接线，`@sse`/`@websocket`/Accept 三种流式信号豁免（防误杀长连流），超时错误显式识别
- **沙箱改多根 allowlist**：显式路径完全信任，`@depends-on` 限制在输入路径目录下（app.rs 与 mock/server.rs 双处修复）
- **capture 逐条化 + `TestResult.warnings` 警告通道**；未定义 `{{var}}` 可见告警；`from_*` 变量名解析修复；`.MD` 大小写路由；URL `://` 子串误判修复；f64 精确相等

### Phase 3：Cookie 加固
- 安全写盘序列（0600 权限、锁内截断、flush 先于解锁）；落盘粒度 per-response → per-file；内联请求 `--env` 环境隔离（新增全局 `--env`）；锁中毒恢复；WS session 错误类型 `RupostError` 化；钻石 DAG 多父 Cookie 冲突显式告警；默认临时会话的用户提示

### Phase 4：流式协议与文档一致性
- SseRunner 事件处理去重（`SseEventContext`，三份复制归一）
- SSE 出错分支 flush 尾帧（TcpListener 断流实测覆盖）
- **Breaking**：空 `data:` 帧不再派发（WHATWG 对齐），`event` 重置、`id` 跨帧留存
- SSE parser 内存上限（buffer/current_data 各 1MB，事件边界复位）
- Markdown 围栏语言取 info string 首 token
- 畸形 JSON 按原文发送 + 警告（可测服务端 400）；快照记录并重放 `@timeout`
- `--env` 路径注入校验；JSON 报告输出 `warnings`；删除死代码（`security.rs`、`metadata_types.rs`）；文档与实现对齐（运算符清单、动态变量实名 `$random_int`、过期路径、虚构的"密钥扫描"宣称移除）

### 质量基线
- **424 个测试 0 失败**（战役起点 318），新增 100+ 测试，其中对抗性回归套件 `tests/adversarial_regression_test.rs` 以 wiremock/裸 TcpListener 承载原始复现命令
- `cargo fmt --check` 与 `cargo clippy --all-targets` 双零

## 遗留与后续（独立项 D1）
- TUI 渲染 `TestResult.warnings`（当前仅 CLI/JSON 报告可见）
- `ws/client.rs`、`ws/frame.rs`、`action_parser.rs` 的 `Result<_, String>` 残留
- Phase 6 架构债：`RunParams` 参数对象、TUI AppState 拆分、rupost-ai client 测试补强、anyhow 移除、`load_generator` 移出 `[[bin]]`
---
