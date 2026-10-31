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

## 下一步：全量测试方案落地（2026-10-25）

测试侧父纲已建立：[2026-10-25-test-strategy-plan.md](doc/plans/2026-10-25-test-strategy-plan.md)（v2，经双子代理交叉审查）。基线 468 测试全绿（317 lib + 6 主二进制 + 145 tests/）。方案 19 功能域矩阵盘点出 P0 缺口 4 项（`generate` 域 0 测试、快照重放仅 1 测试、历史四实子命令 + `export` 桩未锁定、CLI 全局参数位置契约无回归），P1 8 项，P2 5 项。

**P0 执行方案已细化至逐场景级**：[2026-10-25-test-p0-execution.md](doc/plans/2026-10-25-test-p0-execution.md)（13 个新测试，每个含 Given-When-Then 场景说明；4 个 jj 原子提交序列；设计期实证 5 项）。执行期新事实：①子命令模式全局参数双序皆可（AGENTS.md §5 表述过时，随提交 4 修正）；②真实陷阱在内联模式（后置旗被吞且改变解析器选择）；③`history show` help 注释（"1 is the oldest"）与实现（1=最新）矛盾，随提交 3 修正。**待执行**：按 P0 执行方案 §6 提交序列实施，预期基线 468 → 481。

**竞争定位已复审（2026-10）**：[2026-10-25-competitive-landscape-review.md](doc/competition/2026-10-25-competitive-landscape-review.md)。核心结论：TUI"Postman 克隆"路线已红海（Posting 三协议、Nexus 入局、ATAC 掉队、Bruno 下探终端），RuPost 可防御定位是**"面向实时 AI API 时代的终端回归引擎"**——LLM 流式测试闭环（A1）在终端工具中全行业空白且已代码在仓，最高杠杆动作是激活 `crates/rupost-ai` 并补公开示例；gRPC/GraphQL 显式 Won't do（当前），登记 future_backlog。

**全文档整合审计完成（2026-10-26）**：[2026-10-26-project-integration-report.md](doc/dev/2026-10-26-project-integration-report.md)。98 文档三层沉积结构（02 设想/06-07 交付/10 治理），约 40 处矛盾分级（最重：plugin 可行性报告 Phase 1 虚勾选 [x] vs 零代码；dev/progress_summary 自相矛盾）。**关键新发现**：`crates/rupost-ai` 是无引用死代码且与 `src/http/llm_adapter.rs` 双实现悬空——激活 A1 前必须先做合并/接线架构决策，这是与 P0 测试并列的下一优先级。

**AI 激活功能设计完成（2026-10-26）**：[2026-10-26-ai-activation-prd.md](doc/plans/2026-10-26-ai-activation-prd.md)。逐行核实纠正"双实现"误判（crate 资产 = AssertionTranslator 自然语言断言翻译，已带 3 测试；与 llm_adapter 流式职责不同层），架构决策采**方案 A：接线 + 职责文档化**。功能点：`rupost ai assert "<NL>" [--raw]` + 环境变量密钥 + `examples/ai_assert/` 无真实 Key 离线 dogfooding 闭环（mock 后端）+ crate client 0→4 测试；4 个 jj 提交序列；`@ai_assert` 运行时指令显式 Won't do（离线隔离）。

**AI 自举测试方案设计完成（2026-10-26）**：[2026-10-26-ai-bootstrap-proposal.md](doc/plans/2026-10-26-ai-bootstrap-proposal.md)。核心管线 `rupost ai bootstrap --from history|snapshot` 五阶段（素材采集→机械还原→AI 语义增强→引擎校验→回归资产）；四判据：素材自产（历史含全量响应体，recorder.rs 实证）、提炼有界（默认脱敏+4KB 截断+8 运算符解析器兼任 lint）、产物自足（断网可跑，AI 失败降级 HttpGenerator 机械版）、闭环自证（失败回流素材）。L1 已设计（ai assert）→ L2 本方案 → L3 快照转 Mock → L4 远期。执行期 AI 参与/eBPF 录制显式非目标（与 Keploy 分界）。**实施顺序**：AI 激活 Phase 0 → 自举 Phase 1（可穿插 P0 测试补齐）。
---
