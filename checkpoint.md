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

---

## 阶段成果：命令导出插件架构 (rupost export) 交付完成（2026-10-26）

依据插件化架构设计与红队对抗性审查，完整实现命令导出体系：
1. **插件机制与开闭原则**：定义 `CommandExporter` Trait 与 `ExporterRegistry` 注册表，内置支持 `curl`、`httpie` (别名 `http`) 与 `wget` 目标；
2. **安全防线 (Security First)**：
   - POSIX Shell 安全转义与控制字符过滤（单引号 `'...'` 与单行紧凑模式 ANSI-C `$'...'` 转义，终端逃逸防 Pastejacking）；
   - 凭据多维脱敏（请求头敏感 Key 保留 `Bearer/Basic/Digest` 前缀，URL 敏感 Query 参数遮蔽）；
   - 文件覆写保护（`-o/--output` 在文件已存在时阻断并提示 `--force` 覆写）；
3. **核心数据管线 (IR Pipeline)**：
   - 5 层级联变量准备管线与 URL 补全拼接；
   - 自动推断 JSON 请求头（保留用户原始 Body 格式与行序）；
   - 域名/路径匹配自动注入标准 `Cookie` 请求头；
   - 跨请求 `@capture` 符号表静态分析（提供依赖断裂警告并继续导出可用请求）；
   - 自动跳过 `@skip` 块、纯 Mock 路由块与 WebSocket 长连接；
4. **架构解耦与精简**：
   - 将 `generate`、`diagnose` 与 `export` 门面完全下沉，`src/main.rs` 行数从 154 行压降至 131 行（严格遵守 < 150 行红线）；
5. **全面质量验证**：
   - 新增 30 个单元与集成测试（基线测试量提升至 500+ 个，全量 `cargo test` 100% PASS）；
   - `verify_features.sh` 全特性端到端冒烟 100% 通过；
   - `examples/run_all.sh` 15 套内置复杂示例全量回归 100% 通过；
   - `cargo clippy --all-targets` 零告警，`cargo fmt --check` 格式化完全达标；
   - **安全加固微调**：Shell 转义中将控制字符从十六进制 `\xHH` 优化为固定 3 位八进制 `\ooo`，彻底根除 Shell 贪婪消费紧跟十六进制字符的粘连歧义。

**AI 自举测试方案设计完成（2026-10-26）**：[2026-10-26-ai-bootstrap-proposal.md](doc/plans/2026-10-26-ai-bootstrap-proposal.md)。核心管线 `rupost ai bootstrap --from history|snapshot` 五阶段（素材采集→机械还原→AI 语义增强→引擎校验→回归资产）；四判据：素材自产（历史含全量响应体，recorder.rs 实证）、提炼有界（默认脱敏+4KB 截断+8 运算符解析器兼任 lint）、产物自足（断网可跑，AI 失败降级 HttpGenerator 机械版）、闭环自证（失败回流素材）。L1 已设计（ai assert）→ L2 本方案 → L3 快照转 Mock → L4 远期。执行期 AI 参与/eBPF 录制显式非目标（与 Keploy 分界）。**实施顺序**：AI 激活 Phase 0 → 自举 Phase 1（可穿插 P0 测试补齐）。

---

## 阶段成果：质量基线加固 P0 全量测试方案完成交付（2026-10-26）

依据 [P0 测试执行方案](doc/plans/2026-10-25-test-p0-execution.md)（§6 提交序列），4 个原子提交全部高质量交付，全量测试用例数突破 503+：
1. **P0-1 `generate` 域闭环**（Commit `ffb5842a`）：
   - 新建 `tests/generate_integration_test.rs`（4 测试）；
   - 覆盖单条往返格式、多条按最新 N 条切分、真实三段 CLI 接力闭环（`run` -> `generate` -> `test`）、空历史友好退出。
2. **P0-2 快照录制与重放加固**（Commit `9606ecd8`）：
   - 扩充 `tests/snapshot_replay_test.rs`（4 测试）；
   - 验证多请求 URL 改写与 WireMock 真实匹配等价性、`@timeout` 透传计时证明（500ms 超时打断 2s 阻塞）、损坏/非法 JSON 快照优雅容错。
3. **P0-3 历史子命令与 `export` 桩行为锁定**（Commit `0f492833`）：
   - 新建 `tests/history_cli_test.rs`（5 测试）；
   - 锁定 `list`（`--limit`、`--reverse` 顺序反转）、`show`（1-indexed 倒序匹配、UUID/短ID 匹配、越界与未命中容错）、`prune`（基于 mtime 按龄清理旧日志）、`clear`（交互输入拒绝、`--yes`、`--yes --all`）以及 `export` MVP 桩契约；
   - 修正 `src/cli.rs` 中 `Show` 命令帮助注释为 `(1-indexed, 1 is the newest) or UUID`。
4. **P0-4 CLI 全局参数位置契约与内联模式陷阱锁定**（Commit `4ed3c3a9`）：
   - 在 `src/cli.rs` 的 `mod tests` 编写表驱动单元测试 `test_global_args_position_contract_table`（9 个测试用例行）；
   - 实证锁定：子命令模式（`test`, `mock`, `history` 等）clap v4 支持前置与后置全局参数向下继承；内联模式（`rupost GET url`）因 `trailing_var_arg` 会将后置参数吸入 `args`，因此内联模式全局参数必须置于最前；
   - 同步修正 `AGENTS.md` 和 `.agents/AGENTS.md` §5 的规则规范。
5. **自动化三层校验全绿保障**：
   - 全量 `cargo test` 503+ PASS，0 失败；
   - `bash tests/verify_features.sh` 全特性集成冒烟 100% 通过；
   - `bash examples/run_all.sh` 15 套内置用例全量回归 100% 通过；
   - `cargo fmt --check` 与 `cargo clippy --all-targets` 零告警。

---

## 阶段成果：依赖净化治理与 Clean Architecture 参数对象重构完成交付（2026-10-27）

依据技术规格方案，彻底解决长期积累的依赖废弃、错误处理割裂以及长形参坏味道，4 个原子提交全部高质量交付：
1. **Commit 1: 根除废弃的 `serde_yaml`**（`529a45e7`）：
   - Markdown Frontmatter 元数据解析全面统一至原生 `toml`，并实现轻量行扫描回退器兼容遗留 `key: value` 与 `rules: |` 格式，零外部 YAML 依赖；
   - 彻底删除 `serde_yaml` 依赖，消灭 unmaintained 安全漏洞风险。
2. **Commit 2: 根除 `anyhow` 并统一强类型错误体系**（`25be3938`）：
   - `src/middleware/cookie.rs` 与 `src/utils/formatter.rs` 的动态错误全部统一为领域强类型 `RupostError`（新增 `CookieError` 分支）；
   - 从 `src/error.rs` 中剔除 `impl From<anyhow::Error>`，根 `Cargo.toml` 完全移除 `anyhow` 依赖。
3. **Commit 3: Workspace 依赖统一继承与发布包减重**（`2714c8a5`）：
   - 根 `Cargo.toml` 建立 `[workspace.dependencies]`，统一收敛 `tokio`, `reqwest`, `serde`, `serde_json`, `thiserror`, `async-trait`；
   - 子包 `crates/rupost-ai` 依赖全面升级继承 `reqwest 0.13` workspace 依赖；
   - 将 `load_generator` 从正式发布二进制 `[[bin]]` 降级为开发态 `[[example]]`，消除系统 C/OpenSSL 交叉编译污染。
4. **Commit 4: Clean Architecture 参数对象重构与接口下沉**（`d2c1d12e`）：
   - 引入 `RunParams` 和 `ExportArgs` 参数对象，彻底消除 `cli::run`（7个参数）与 `run_export_from_cli`（12个参数）的长形参列表；
   - 废弃冗余过渡函数 `run_export_from_cli`，直接由机制层 `run_export(opts)` 接管；
   - `src/main.rs` 从 131 行进一步精简至 116 行（严格控制在 150 行上限之内）；
   - 修正架构文档 `.agent/skills/rupost-architecture/SKILL.md` 中遗留的 `fs2` 描述为 `fs4`。
5. **Commit 5: 构造器精进与 Builder 模式补全**（`ce324f84`）：
   - 为 `RunParams` 实现 `Default`（绑定 `"http"` 默认 scheme）与通用泛型入参构造器 `RunParams::new(["GET", "..."])`；
   - 提供全套强类型链式 Builder 方法（`with_env`, `with_default_scheme`, `with_cookie_file`, `with_no_cookies`, `with_debug`, `with_debug_on_failure`）；
   - 为 `ExportArgs` derive `Default`；
   - 补充 `test_run_params_builder_and_default` 单元测试，进一步提升测试和库级易用性。
6. **自动化三层校验全绿保障**：
   - 全量 `cargo test --workspace` 350+ 单元测试与全部集成测试 100% PASS；
   - `bash tests/verify_features.sh` 全特性集成冒烟 100% 通过；
   - `cargo fmt --check` 与 `cargo clippy --workspace --all-targets` 零告警。

---
