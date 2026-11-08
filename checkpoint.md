# RuPost 质量加固战役 Checkpoint 归档（2026-10 ~ 2026-11）

## 最新里程碑进展 (2026-11-08)
- **Web 工作台样式升级与纯 ASCII 架构重塑 (`feature/web-ui-redesign`)**：
  1. **Studio Pro 纯 ASCII 视觉体系**：全面清除页面中所有 Emoji 图标，以专业工程符号（如 `[R]`, `[DOC]`, `[PASS]`, `[FAIL]`）替代；基于深空碳黑与精细刻线重构三栏网格。
  2. **代码编辑器增强**：自研轻量级行号槽（Gutter）滚动同步、光标行列度量、`Ctrl+S` 快捷保存、`Shift+Enter` 单块快捷执行、`Ctrl+I` 快速导入。
  3. **Runtime Inspector 控制台重构**：右侧调试面板升级为 Tab 分页架构（Overview / Body / Headers / Assertions / Waterfall / Raw），支持响应体一键复制、JSON 缩进格式化与毫秒级时延瀑布图渲染。
  4. **双主题架构落地 (Dark & Light Theme)**：基于 CSS Custom Properties 实现深色 (Deep Obsidian) 与浅色 (Studio Paper) 动态热切换，支持系统深浅偏好自适应检测与 `localStorage` 持久化，顶部状态栏提供 `[THEME: DARK/LIGHT]` 切换动作。
  5. **四层测试金字塔与全功能验证 (100% PASS)**：
     - **单元与集成测试**：534 个 `src/lib.rs` 单测 + 14 个 `tests/web_api_integration_test.rs` 全部通过。
     - **全特性冒烟测试**：`tests/verify_features.sh` 涵盖 Mock 变体、网络诊断瀑布条、DAG 拓扑、AI 机械降级、cURL 导入等 100% 通过。
     - **全量内置示例回归**：`examples/run_all.sh` 15 套真实复杂用例（SSE 流式断言、变量级联、状态传递）全部通过。
     - **浏览器 CDP 自动化端到端测试**：通过真实 Headless Chrome 执行 `test_web_all_features.py`，覆盖主题切换、文件树过滤、行号槽双向滚动、原位单块调试、套件执行、Tab 控制台切换、向导装入、AST 语法守卫与无 Emoji 规则等 **22 项功能全部 PASS (22/22)**。
     - **静态代码质量**：`cargo clippy --tests -- -D warnings` 零告警通过。

## 历史里程碑进展 (2026-11-06)
- **全文档体系重构、内容汇总与分类规整 (Clean Docs & Features Dossiers)**：
  1. **7 大核心领域三段式技术档案落地 (`doc/architecture/features/`)**：围绕“① 需求是什么、② 演进决策与核心算法、③ 当前落地代码与测试映射、④ 后续演进规划”四大维度，深度整合并提炼了 `cookie_and_session`、`batch_and_workflow`、`ai_engine_system`、`debug_and_diagnostics`、`protocols_and_mock`、`tui_architecture`、`plugin_and_extensibility`，彻底消解早期草案碎片。
  2. **`doc/plans/` 演进规划大熔炉规整**：100% 物理保留历史 50+ 篇 PRD 原文，按 `2026-q2/`、`2026-q3/`、`2026-q4/` 季度里程碑归档；在 `doc/plans/README.md` 中构建了 5 大业务演进主线（AI 智能线、TUI 终端交互线、执行调度线、排障重放线、质量战役线）的交叉引用矩阵。
  3. **全局知识中枢 `doc/README.md` 建立**：确立 Diátaxis 与 Clean Architecture 文档组织标准，保持 Makefile、AGENTS.md、README.md 外部契约 100% 兼容无损，全量 512 个单测与端到端 `verify_features.sh` 持续高标准 PASS。

---

## 历史状态总结 (Historical Status)

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

## 阶段成果：终端 API 竞品深度剖析与演进启示文档交付（2026-10-27）

依据用户要求，针对终端生态三大代表性工具 **ain**、**ATAC**、**slumber** 进行了深入的架构与哲学剖析，完成高规格战略分析文档 [2026-10-27-terminal-api-clients-ain-atac-slumber-deep-dive.md](doc/competition/2026-10-27-terminal-api-clients-ain-atac-slumber-deep-dive.md)：
1. **哲学解构**：Unix 纯粹管道流（ain）、全功能 Postman 终端复刻（ATAC）、声明式配方驱动的优雅 TUI（slumber）；
2. **护城河巩固**：论证 RuPost 在“活文档（Markdown 一等公民）”、“实时 AI API 流式闭环（全行业空白）”、“DAG 确定性编排”以及“Mock 与用例同源”上的绝对优势；
3. **吸收路线图**：规划 OSC 52 终端剪贴板、Shell 动态命令求值 `$(cmd)`、单请求惰性依赖自愈与 OpenAPI/Postman 逆向转换三阶段转化。

---

## 阶段成果：AI 失败智能处理 PRD 规格制定交付（2026-10-27）

依据竞品对标与补充切入点分析，完成两大绝对蓝海能力的产品 PRD 制定：[2026-10-28-ai-failure-triage-and-attribution-prd.md](doc/plans/2026-10-28-ai-failure-triage-and-attribution-prd.md)（v1.0）：
1. **E2 断言失败就地 AI 归因 (In-place Attribution)**：嵌入 `rupost test --ai-explain` 失败回显，无需切换窗口，直接就地生成根因假设与修复指引；
2. **E5 CI 批量失败聚类 Triage (Failure Clustering)**：新增 `rupost ai triage <report.json>`，利用结构化 JSON 报告将成百上千条失败聚合为 2~4 个根因簇；
3. **Clean Architecture 设计**：脱敏抽取器 + 领域分析器 + 离线 Mock 闭环设计，杜绝凭据泄漏，全流程 100% 具备确定性离线回归能力。



---

## 阶段成果：AI 激活——`rupost ai assert` 创作辅助命令交付完成（2026-10-27）

依据 [AI 激活 PRD](doc/plans/2026-10-26-ai-activation-prd.md)，4 个原子提交全部交付，休眠 crate `rupost-ai` 正式接线激活（终结"workspace 成员但主包零依赖、全库零 use rupost_ai"的死代码状态）：
1. **Commit 1 接线**（`5603a0c8`）：根 `Cargo.toml` workspace 依赖 + `src/cli.rs` `Ai`/`AiCommands` 子命令 + `src/main.rs` 分发。密钥仅从环境变量读取（`RUPOST_AI_API_KEY` → `OPENAI_API_KEY` 回退），缺 Key 报错含可执行指引并 exit 1；后端错误响应体 500 字符截断，密钥与 prompt 原文不进任何输出路径。
2. **Commit 2 测试**（`4befd1d0`）：crate 侧 wiremock 4 测试（成功解析 / 401 ApiError 透传 / 空 choices / 非 JSON 体）+ translator 存量 3 测试保持全绿；二进制级 `tests/ai_assert_test.rs` 3 测试——**离线 dogfooding 闭环**（`rupost mock` 充当 OpenAI 兼容后端，零真实 Key）、缺 Key 指引、后端 401 状态码透出；`examples/ai_assert/mock_ai.json` 提供固定 chat completion 响应。
3. **Commit 3 示例与脚本**（`b2f8dfed`）：`examples/ai_assert/README.md`（零 Key 演示 + Ollama 指引）+ `verify_features.sh` 步骤 7（dogfooding 闭环 + `set -e` 下退出码断言）。
4. **Commit 4 文档对齐**：`directory_structure.md` 纠正 translator.rs 的历史误判描述（"流式事件帧规整"实为 `llm_adapter` 职责）并登记 crate "AI 创作辅助" 定位（分层职责与 llm_adapter 互不依赖）；README 特性区、CHANGELOG Added、progress_summary、checkpoint 同步归档。
5. **质量基线**：`cargo test --workspace` 全绿（首次覆盖 crate，新增 7 测试）；`cargo fmt --check` / `cargo clippy --all-targets --all-features -- -D warnings` 零告警；`verify_features.sh` 全特性回归 PASS。

---

## 测试方案 v3 追平现实（2026-10-28）

[全量测试方案](doc/plans/2026-10-25-test-strategy-plan.md) 已更新至 v3，追平至 HEAD（`f61b3556`）：

- **基线翻新**：468 → **551 测试 / 34 套件 / 0 失败**（主包 543 = lib 356 + bin 9 + tests/ 178·29 文件；crate 8）。wiremock 14 文件、TcpListener 5 文件。
- **v2 P0 全部核销**（§6.1）：generate 8 测试（含凭据模板化泄露回归锚点 dc1f048c）、快照重放 4、历史 CLI 5（show help 修正同交付）、CLI 位置契约表（AGENTS.md §5 同步修正）；`--debug-on-failure`（原 P1）随 DBG-2 落地。实际交付 31 测试 > 方案 13。
- **矩阵 19→20 域**：新增 AI 创作辅助域（`ai assert` 已锁 / `ai bootstrap` 测试包随实施交付，lint 表驱动+脱敏器边界为密度最高处）；域 15 吸收 `--only`/`path#n` 单请求重跑（8 测试）。
- **新方法论缺口（实证）**：默认 `cargo test` 不覆盖 workspace 子 crate 的 8 个测试——纪律升级为 `cargo test --workspace`；CI 门禁升级列 P2。
- **残余真缺口（P1 六项）**：SSE 断流/1MB 上浮集成层、capture 专项、并行×`--cookie-file` 契约、JSON 报告 schema、TUI `state/stream.rs` 流式渲染、TUI warnings 渲染。

---

## HTTPS 回归修复（2026-10-29，指引测试会话发现）

按测试方案 v3 指引执行全链门禁时，`examples/run_all.sh` 14/15 失败（外网可达却 0ms 报 "error sending request"）。根因：`2714c8a5` workspace 依赖统一时 reqwest 加了 `default-features = false` 但未补 TLS feature——**HTTPS/WSS 静默失效，且全量 551 测试全绿（本地 http wiremock 无法捕获）**，暴露了"构建配置漂移不可测"盲区。修复：workspace 定义补回 `default-tls`（恢复原行为）+ load-bearing 注释锚。验证：https 直连 200、workspace 551 全绿、run_all.sh 15/15。**启示**：run_all.sh 是目前唯一能抓住 TLS 缺失的探针（外网依赖使其不可入 CI），发版前必跑地位加固。


---

## 测试缺口清偿、P2 卫生批次与双轮审查（2026-10-29/30）

三个 workspace 批次 + 两轮双子代理对抗审查，测试策略 v3 的 P0/P1/P2 **独立台账全部清零**：

- **tui-fix（10-29）**：TUI 测试缺口清偿（stream 流式域 0→10 单测、warnings 全链路渲染 [D1 关闭]、预加载真函数化、主题轴冒烟）+ 双审查修复轮（buffer 级消费端守护、缓存面板样式丢失修复、流式视图 Warnings 摘要）。
- **test-gaps（10-30）**：§6.2 四项 P1 真缺口清偿——并行×`--cookie-file` 落盘契约（串行逐文件/并行 Ephemeral 不落盘）、SSE 韧性上浮真实网络层（断流 flush/1MB 截断×2）、JSON 报告 schema 精确键集合（含 `diagnose --report json`）、capture 专项（逐条降级/from 解析/global Arc 共享）。
- **p2-hygiene（10-30）**：CI test/clippy/build 三处 `--workspace`（38 vs 36 套件实证）、`--env-file` 无效路径静默忽略契约锁定、`make coverage` 一键入口、criterion benches 验证、Windows 三平台核对；DX-1/3/4 落地（Makefile 九目标 [gate-fast 上线即抓到未格式化测试]、PRD 模板 `make prd`、SDK 检测 `make build`）。
- **review-fixes（10-30）**：两轮审查（4 P1 + 9 P2）全部闭环——RST 传输层错误真守护（SO_LINGER=0 + socket2 0.6）、cookie 逐文件粒度子进程挂起观测点（persist 移出循环实证变红）、失败形态 schema、SDK 切换分支可达性（改探固定 Xcode 路径）；过程事故（红检查回写静默失败、divergent）如实归档并收敛，终态 src/ 零变更。

**质量基线**：`cargo test --workspace` 38 套件全绿；clippy `--workspace --all-targets --all-features` 零告警；`make gate` 成为本地门禁标准入口。

---

## TUI 流式调试深化（2026-10-30，tui-streaming workspace）

PRD 驱动四域交付（SSE/WS 接入 + 授权 + 消息搜索），全程 TDD：
1. **变量上下文**：TUI 启动与 CLI 同源加载（toml/.env/系统变量 + `--env/--var/--env-file` 透传）——终结授权变量不解析的断链；
2. **快速授权**：`A` 键模态 → `__quick_auth` 注入（显式优先决策表锁定）；
3. **消息搜索**：`/` 三视图高亮 + n/N 环绕跳转；顺带修 `response_total_lines` 计数 bug；
4. **WS 交互会话**：`W/S/D` 常驻会话（复用 WsSession，零侵入 runner 剧本执行），echo 回环 + 授权握手 + 关闭通知三集成测试。

质量基线：39 套件 592 测试绿；clippy 零警告。PRD 非目标：SSE 事件结构化、多会话并行、SSE 交互发送。
## 阶段成果：AI 失败智能处理——E2 就地归因与 E5 失败聚类交付完成（2026-10-28）

依据 [E2/E5 PRD v1.1](doc/plans/2026-10-28-ai-failure-triage-and-attribution-prd.md)、[统一执行计划](doc/plans/2026-10-28-ai-e2e5-master-execution-plan.md) 与 [实施手册 v1.2（含 Gap Review 15 项）](doc/plans/2026-10-28-ai-failure-triage-and-attribution-implementation-plan.md)，5 步原子提交全部交付：
1. **Step 0 地基**：`run_ai_assert` 下沉 `src/ai/mod.rs`（main.rs 182→127 行）；`src/ai/config.rs` 环境变量配置中心（空 Key 视同未配置）；`src/ai/redactor.rs` 统一脱敏截断器（复用 exporter::redact + JSON 递归键掩码 + is_char_boundary UTF-8 安全截断）。
2. **Step 1 报告增强**：`JsonFileReport` 强类型契约（新增 expected/actual/stream_event_index 与失败项脱敏 512B 响应片段，旧字段名零变更）+ `--report-file` 落盘 + `TestSuiteOptions::default()`（cookie 默认语义与 CLI 一致）。
3. **Step 2 领域层**：crate 新增 `json_util`（宽容 JSON 提取）、`attribution`（E2 归因，serde alias 兼容 PRD 命名）、`triage`（`FailureCategory` 无损枚举反序列化 + **分桶 ID 守恒回填**：重复/幻觉 ID 忽略、漏桶自动补确定性簇，计数 100% 守恒）。
4. **Step 3 接线**：`src/ai/rules.rs` 确定性基线（6 类原因推导表 + URL 模式签名 + 20 桶溢出合并 + failure_signature 去重键）；`explain.rs`（签名去重 + Top-3 配额 + 连续 2 错熔断）；`triage.rs`（内存/文件双入口、无 Key 黄色警告、--json/--format）；CLI `--explain/--ai-explain/--triage` 与 `ai triage` 子命令；B0 引擎零 AI（`rg rupost_ai src/runner/ src/http/ src/parser/ src/assertion/` 为空；AI 编排仅 app.rs 渲染阶段挂载 `crate::ai`，`redactor` 纯文本工具另被 reporter 复用）。
5. **Step 4 闭环**：`examples/ai_triage/`（mock 双路由 + failing fixture）+ `tests/ai_triage_test.rs` 4 用例（确定性分桶/wiremock AI 增强/E2 无 Key 降级 exit 1/**dogfooding 闭环**：`test --report-file` → `ai triage` 消费真实报告）+ verify_features.sh 步骤 8 + 文档归档（CHANGELOG/directory_structure/progress_summary/checkpoint）。
6. **质量基线**：全量 570 测试 0 失败（crate 新增 17 测试）；fmt / clippy --all-features -D warnings 双零；verify_features.sh 全特性冒烟 PASS；全程零新增第三方依赖；Gap Review 中 GAP-1 经复核证伪（re-export 已存在，文档已标注禁止重复添加）。

### 子代理交叉审查与修复（第二轮，2026-10-28）

两路子代理（正确性/安全 + 架构/测试质量）独立审查并实证复现问题，全部闭环：
1. **P1 脱敏旁路封堵**：reqwest 错误文本自带完整原始 URL、敏感键断言的 actual/expected/message 明文穿透报告与 LLM 上下文（审查实测复现真实 token 外泄）。新增 `redact_free_text`（散文内嵌 URL 走 redact_url query 掩码）与 `redact_assertion_value`（断言 raw 命中敏感键 → 值整体 [REDACTED]；否则 URL 掩码 + 200 字符截断），在报告生产（reporter）、E2 上下文（explain build_context）、E5 sample_error（rules）三处消费点统一应用，附 canary 回归测试。
2. **P1 测试竞态**：三个集成用例争抢固定端口 9002（并行 harness 下结构性 flaky）——改每用例独立端口 + fixture `{{base_url}}` 变量化 + `--var` 注入 + 临时文件带进程 ID。
3. **E2 验收补全**：PRD US1 无 Key 黄色提示（原被静默丢弃）——`ExplainSession` 客户端注入化（`explain_failure_with_client`），补配额（5 签名仅 3 调用）/熔断（连续 2 错停调用）/去重单测（StubClient 计数）；移除恒真常量测试。
4. **签名稳定性**：`failure_signature` 对 error 文本内 URL 归一 `<url>`——同端点异实例同签名，E2 去重对网络错误真正生效。
5. **文档如实修订**："唯一挂载点"声称（redactor 被 reporter 复用属纯文本工具）、DoD 阈值（≥590→571 实测修订）。
审查确认项：B0/B3 硬不变量成立、守恒算法/降级链路/零外网闭环/canary 泄漏检测设计扎实。修复后全量 578 测试 0 失败，clippy --all-features -D warnings 零告警。
### 复核轮（子代理安全复核，2026-10-28）

独立复核员验证 P1 修复闭环：**两处 P1 主泄漏通道在全部三个消费点确认修复并有测试锁定，结论"P1 闭环成立，可合入"**（579 测试全绿、防御性二次脱敏幂等、签名归一无交叉漏洞、redact_free_text 正则无 ReDoS）。同步登记 3 个**预存在、条件性**后续工单（不阻塞本次合入）：
- **P2-1** `diagnose_report.url` 未脱敏旁路：`--diagnose/--debug` + query 密钥 URL 时，`JsonTestReport.diagnose_report` 无条件序列化原始 URL（`src/reporter.rs`、`src/http/diagnose.rs:22`）——建议 redact_url 或条件 skip；
- **P2-3** 断言 `raw` 为变量解析后文本：`{{expected_token}}` 类模板会把密钥字面量带入 raw → 报告/LLM prompt/聚类标题（`redact_assertion_value` 只掩 value 不掩 raw）；
- **P2-2** 外部不可信报告防御纵深：`ai triage <手写报告>` 路径下 error/url/断言值依赖生产方脱敏（`context_from_json_test_report` 防御层未覆盖断言值与 default_root_cause）。
另有低优先级：`tokenize` 等子串误报（安全侧偏移）、词表缺独立 `auth`/`session`、非 JSON 体 Bearer 字面量不掩。



## 阶段成果：rupost-ai 能力线交付与重建归位（2026-11-01/02）

依据 [E2/E5 PRD v1.1](doc/plans/2026-10-28-ai-failure-triage-and-attribution-prd.md)、[统一执行计划](doc/plans/2026-10-28-ai-e2e5-master-execution-plan.md)、[F7/F8 设计深潜](doc/plans/2026-10-28-ai-f7-f8-design-deepdive.md)，能力线四特性 + importer 全部交付（经污染侧线重建 + 双子代理审查三轮）：
1. **E5 失败聚类**：signature-bucket 确定性分组（≤20 桶、溢出合并、守恒断言）+ `ai triage <report.json>`（`--json/--format`，强类型 JsonFileReport 反序列化契约，0 失败短路跳过 B4 解析）；
2. **E2 就地归因**：`test --explain` 规则基线 + AI 解释（熔断/配额/签名去重、`RUPOST_AI_EXPLAIN` 环境开关），与 `--triage` 同源走 B4 配置链；
3. **F7 resolve / F8 bootstrap**：`ai resolve` 原地展开 `@ai-assert`（无指令早退、Top-20 配额、熔断）；`ai bootstrap --from history` 五阶段管线（TestSynthesizer 分块 + 熔断 + 防覆盖，`-d ''` 空体与 `@file` 显式拒绝为 importer 边界）；
4. **reporter 强类型契约**：JsonFileReport/JsonTestReport/JsonAssertionReport（键集恒定无 skip、diagnose url 脱敏、warnings 脱敏、断言三元组 expected/actual/stream_event_index）；
5. **redactor 加固**：URL userinfo/OAuth fragment 掩码、非 JSON 体键值兜底掩码（Bearer scheme 前缀）、外部报告信任边界幂等再脱敏——均有单测锁定。

**重建与评审**：污染侧线（.jjconflict 快照 + 陈旧写入）不可逐提交恢复，在链头重建并按文件边界拆为基建/接线/文档三提交；双子代理审查三轮（完整性审计、质量/安全、增量+终审），6 P1 + 10 P2 + 3 CONCERN 全部闭环。

**质量基线**：43 套件 735 测试绿；clippy `--workspace --all-targets --all-features -- -D warnings` 零告警；fmt 通过。

---

## 阶段成果：DX-Debug R7 失败钩子与 trace 锚点协议交付完成（2026-11-03）

依据 [DX-Debug PRD](doc/plans/2026-10-28-developer-debug-experience-prd.md) US-R7 / 主执行文档 T6，调试链 D5（深挖取证）断裂带收口——R1–R10 中最后一个未实现的确定性需求落地。基线：rebase 后的 `feature/ai-capability` 线（main 尖 + AI 能力蒸馏栈）。

1. **锚点协议**：`rupost t --on-failure <CMD>`——每个失败请求触发一次用户 shell 命令，注入 `RUPOST_FAILED_URL`（redact_url 脱敏，敏感 query 掩码、路径保留）/ `RUPOST_FAILED_STATUS` / `RUPOST_FAILED_TRACE_ID`（`Traceparent > X-Request-Id > X-Trace-Id` 优先级，非 UTF-8/空白值跳过续找）。rupost 只给锚点，服务端日志聚合委托 stern/k9s 用户管道（非目标红线维持）。
2. **进程语义**：`src/runner/on_failure.rs` 新模块（B0 确定性零 AI）——`tokio::process` + 10s kill_on_drop 超时；钩子 stderr/stdout 一律并入 rupost 的 stderr 透传（保 `--report json` stdout 机器可读纯净）；钩子超时/非零退出/未检出 trace 头（空串 + `TestResult.warnings` 可见）均不改写测试退出码，不阻塞 Fail-Fast。
3. **编排位置**：钩子环节置于渲染前——"无 trace 头"warning 同步落入 JSON 报告 `warnings` 数组与 `--explain` 上下文；`FailureContext` 增 `trace_id`（serde skip absent；JSON 报告摄入路径因 schema 锁定保持 None，仅实时运行路径可得）。
4. **验证**：单测 9（trace 决策表 4 + 进程契约 5，unix 门控保 windows CI）+ 集成 5（锚点三元组捕获/空串+warning/钩子失败隔离/成功零触发/JSON 模式）+ `verify_features.sh` 步骤 11（mock 401 + X-Request-Id → 假脚本捕获 `anchor-42`）。全量 750 测试 0 失败，clippy `--workspace --all-targets --all-features` 零告警，fmt/verify_features PASS。

### 审查修复轮（2026-11-03，双子代理审查启动后先行修复的两个自证缺陷）

1. **P1 `--report json` stdout 污染**：钩子 `print!("{}", stdout)` 发生在 `report_json` 写 stdout 之前——钩子任何 stdout 输出（如 stern 摘要）混入 JSON 报告流，机器解析报废（实测复现：stdout 首行为 `POLLUTANT-STDOUT`）。修复：钩子 stderr/stdout 一律并入 rupost 的 stderr（PRD 仅约定 stderr 透传；终端用户仍可见全部输出），集成测试 `test_hook_output_never_pollutes_json_stdout` 锁定纯净契约。
2. **P2 `HookOutcome.timed_out` 死分支**：`Ok(Err(_)) | Err(_)` 合并臂使 `timed_out` 恒为 false，超时提示永不触发。修复：拆分超时臂（`Err(_)` → `timed_out: true`）与等待期 IO 错误臂，单测断言旗标如实上报。

**过程记录**：会话开头完成两项重组——①`feature/ai-capability` 蒸馏线 rebase 至 main 尖（bookmark 自动前移）；②rupost-ai 工作区自 TUI 线尖切换落位其上。调试链现状：D0–D7 全通，R 清单仅余 R3（diagnose JSON 契约测试债，fast-follow）与 R6（replay per-entry，随 T4）。

> **2026-11-04 勘误**：上文 R7 节标题"R1–R10 中最后一个未实现的确定性需求落地"与本段"仅余 R3/R6"并存系措辞张力（R7 交付时 R1/R2 已于主线先行、R6 亦为未实现的确定性需求，"最后一个"与"仅余"口径不一）。本日 R6 + T5 搭车 + R3 壳层已交付（见 progress_summary 批次行）——**确定性调试链（R1–R7/R10）就此全绿**，R 清单剩余项均为 AI 增强型（R8/R9 已随本线交付）。另注：本日顺带修复 R7 审查轮孙进程测试的 macOS 可移植性（BSD `sleep` 拒绝附加参数致 fork 不发生）。

## 阶段成果：P1 批次交付（T4 Smart Diff + AI 接线 + T1/T2 生成语义线，2026-11-04）

PRD（双子代理评审 6 P0 吸收）后五原子提交：S1 replay body 深比较（默认 14 键排除 + `--ignore-fields`/`--status-only` 逃生 + 第四形态 body-only exit 1）；S2 `replay --explain`（ExplainSession FailureContext 级入口 + 桥合成 body 证据，无 Key B1 降级）+ verify_features 步骤 13；S3 T1 动态值反向模板化（dynamic.rs 形状扫描 + resolver 新增 `{{$timestamp_iso}}`/`{{$timestamp_ms}}`，敏感键凭据模板化优先）；S4 T2 链式值溯源（trace.rs 反序就近 + Cookie 删头靠罐 + 管线序 T2>T1>凭据）+ wiremock 链式闭环；S5 台账。全量门禁绿 + verify_features 13 步 PASS。**过程记录**：评审"零翻红"实证漏网两处（R6 闭环测试、verify 12.5）为新语义正确触发，对齐测试；verify 步骤 13 曾插入 HIST_SNAP 清理之后（bash -x 实证重排）。调试试跑集 examples/debug_demo 与统一指南同步可用。

### Secrets Masker body 残余（2026-11-04，随 P1 批次后同日交付）

路线图条目 1 勘误后残余（header/query 已由 generate 凭据模板化与 export 脱敏覆盖）：`t --save-snapshot` 与 `history export` 新增 `--mask-secrets`（opt-in）——headers 走 `redact_header_value`（Bearer 前缀保留）、body 走 `mask_body_secrets`（敏感键 `[REDACTED]` + PII 三正则手机/银行卡/邮箱，无截断）；掩码产物 schema 不变、仍可 replay 加载。**D1 翻案**：默认关（T4 body 比对上线后默认掩码毒化重放保真度）。掩码原语 `redact_free_text`/`mask_inline_secrets` 自 ai/redactor 下沉 exporter/redact（B0 统一归位）。PII 决策表 + 双命令集成（掩码 vs 无旗标保真对照）+ 掩码快照 schema 可加载，全量门禁绿。

---

## TUI 接入 CLI 能力（2026-11-05，tui-cli-bridge workspace）

PRD 驱动（`doc/plans/2026-11-05-tui-cli-parity-prd.md`）三域九项：执行模型对齐（多请求 streaming、选择器、cookie bridge）、调试增强（timing 行、Diagnose tab、X 导出）、快照与生成（Ctrl+S、G 键）。executor 新增 `execute_all_streaming` 委托重构（行为零变化）。附带诊断 flake 修复（透明代理环境哨兵）。

---

## TUI 接入剩余 CLI 功能（2026-11-05/06，tui-cli-bridge workspace）

"接入所有新增功能"目标收尾：诊断 flake 修复（环境哨兵）→ CLI-parity 三域九项（前批）→ 剩余四项（V 快照重放 / M mock 开关 / I AI 断言 / --cookie-file 持久化）。mock/server.rs 拆分 build/serve 供编程接入。基线 45 套件 807 测试绿；clippy 零警告。


---

## TUI 组件化重构与冲突清理战役（2026-11-06，tui-tests-full 分支）

完成 TUI 历史冲突整理、全量回归验证、Code Review 修复及 UI 模块组件化重构：

### 1. 提交树整理与冲突根除 (Conflict Cleansing)
- **基线对齐**：以稳定的 `sntzqrmv (tui-cli-bridge)` 为基准，将包含 3 个历史冲突提交的旧链原子化重构为 3 个标准清晰提交（`feat(tui)`、`test(tui)`、`docs(tui)`）。
- **废弃脏提交**：彻底 `abandon` 旧冲突链（`wvwymprs::lowvxnxu`）及历史悬空冲突 `mpyyoknn`，同步更新辅助工作区 `.workspace/tui-tests-full`。
- **冲突清零**：`jj log -r 'conflicts()'` 当前全仓库 0 冲突。

### 2. Code Review 缺陷手术式修复 (Surgical Fixes)
- **UX 语义纠正**：修复 `save_current_snapshot` 成功时误写 `last_error` 导致状态栏飘红的缺陷，成功时置 `state.last_error = None`。
- **渲染零复制优化**：消除 `src/tui/ui/mod.rs` 中 `show_export.clone()` 在每帧渲染循环中的深层堆内存分配，改为借用零拷贝渲染。
- **代码卫生治理**：清理 `src/tui/handlers/editor.rs` 中重复与错位的注释。

### 3. 第一性原理组件化解耦 (UI Componentization)
- **布局抽象**：新增 `src/tui/ui/popups.rs`，提取 `centered_rect` 与 `centered_rect_exact` 无状态居中算法，消除 4 处重复布局数学运算。
- **弹窗组件化**：将 Help、Unsaved Confirm、Export、Modal Input 弹窗统一收敛至 `popups` 模块，遵循纯函数只读入参契约。
- **状态栏解耦**：新增 `src/tui/ui/status_bar.rs`，独立承载错误提示、认证与 Mock 会话指示、上下文敏捷按键映射。
- **主入口极简**：`src/tui/ui/mod.rs` 从 589 行压缩至 ~180 行（净减 400+ 行），蜕变为纯粹的布局分发 Orchestrator。
- **离屏测试加固**：新增 4 组离屏 `TestBackend` 组件级单元测试，全量 52 个冒烟测试 + 8 个特性保障测试全部 PASS，clippy 零告警。

### 下一步计划 (Next Steps)
- 探索 `src/tui/ui/response.rs`（854 行）的 Tab 子面板组件化拆分。**【2026-11-08 勘误】**：原表述"5-Tab（Timing/Diagnose/Headers/Body/Stream）"已过时——实际 `DetailTab` 枚举为 Response/Headers/Request/Results/Diagnose，Timing 内联于 Response 臂；权威口径见 [解耦提案 §3 靶点三](doc/plans/active/2026-11-08-codebase-decoupling-and-monolith-refactoring-proposal.md)。
- 完善 Web 界面 / 跨平台终端自适应交互细节。

---

## 场景 C（AI/SSE + WebSocket 协同）规划与测试落地（2026-11-07，role-alignment-scenario 工作区）

依据四大调试模式（CLI 深度诊断、TUI 交互面板、双角色本地 Mock、网络瀑布诊断）完成场景 C 的全套规划、规范契约与离线端到端测试用例：

### 1. 计划与规范归档
- **规划文档**：保存至 [2026-11-06-scenario-c-ai-collaboration-testing-plan.md](file:///Users/zsyzzx/project/rust/rupost/.workspace/role-alignment-scenario/doc/plans/active/2026-11-06-scenario-c-ai-collaboration-testing-plan.md)。
- **契约定义**：新增 [ai_spec.md](file:///Users/zsyzzx/project/rust/rupost/.workspace/role-alignment-scenario/examples/scenarios/scenario_c_ai_collaboration/ai_spec.md)，定义 `@sse` 流式打字机（包含 200 流式分块与 429 限流降级分支）以及 `@websocket` 协同白板剧本。
- **回归套件**：新增 [ai_test_suite.http](file:///Users/zsyzzx/project/rust/rupost/.workspace/role-alignment-scenario/examples/scenarios/scenario_c_ai_collaboration/ai_test_suite.http)，声明式断言 Token 流、429 错误体与 WebSocket 双向握手/收发/关闭。

### 2. 自动化集成测试闭环
- 在 [tests/scenarios_integration_test.rs](file:///Users/zsyzzx/project/rust/rupost/.workspace/role-alignment-scenario/tests/scenarios_integration_test.rs) 中实现场景 C 专属用例：
  1. `test_scenario_c_spec_structure_and_mock_compilation`：验证 `@sse`、`@stream_to`、`@websocket` 等元数据与 Mock 变体编译。
  2. `test_scenario_c_suite_full_execution_against_mock_servers`：拉起本地 AxumMockServer 与内存级 WS 协同虚拟服务（带高频心跳软过滤），完整验证 SSE 逐行推流、增量 LLM 合成文本断言、429 降级断言、WebSocket 动作序列匹配与 `@stream_to` 实时落盘。
- **全量回归保障**：513 单元测试 + 全量集成测试 100% 绿灯，clippy 零告警，fmt 格式合规。

---

## Stage 2（Web Phase 0 静态 HTML 报告与时延瀑布图）落地完成（2026-11-07）

依据 Clean Architecture 原则与第一性原理，完成零外部网络依赖、单文件自包含、高审美现代化 Dashboard 静态 HTML 报告生成管线：

### 1. 领域模型与开闭接口
- 新增 `src/report/` 模块：
  - `src/report/types.rs`：定义归一化领域视图 `ReportContext`、`ReportSummary`、`ReportRequestItem`、`TimingWaterfall`，实现从执行结果到视图上下文的解耦转换，内置防 XSS 的安全转义函数 `escape_html`。
  - `src/report/generator.rs`：抽象开闭接口 `ReportGenerator`，统一支持 HTML、JSON 及未来多目标渲染。
  - `src/report/html.rs`：实现高审美 `HtmlReportGenerator`（极简暗色 HSL 调色、Summary KPI 卡片、进度条、Failed-first 树形卡片、断言对比证据链、以及原生 CSS 时延瀑布图）。

### 2. 执行流与 CLI 接入
- 在 `src/runner/app.rs` 中集成 HTML 报告输出，当传入 `--report html` 或 `--report-file <path.html>` 时，自动渲染并生成自包含单文件。
- 在 `src/cli.rs` 中扩展文档说明。

### 3. 测试与验证闭环
- 新增集成测试 [tests/html_report_integration_test.rs](file:///Users/zsyzzx/project/rust/rupost/.workspace/role-alignment-scenario/tests/html_report_integration_test.rs)：
  - `test_e2e_run_test_suite_generates_html_report`：真实命令行套件端到端导出自包含 HTML，断言内联样式与极简控制脚本。
  - `test_e2e_html_report_with_mock_and_waterfall`：本地 Mock 真实发包并生成时延瀑布图（`wf-dns`、`wf-tcp`、`wf-ttfb`）及断言证据。
  - `test_offline_json_report_to_html_generation`：验证脱机从 JSON 报告二次渲染 HTML。
- 全量回归：516 单元测试 + 全量集成测试 100% 绿灯，clippy 零告警，代码格式合规。

---

## Stage 3 & 4（Web Phase 1 活文档工作台、自定义扩展架构与 Spec 脚手架）落地完成（2026-11-07）

依据 [2026-11-07-web-workbench-and-customization-architecture.md](file:///Users/zsyzzx/project/rust/rupost/.workspace/role-alignment-scenario/doc/plans/active/2026-11-07-web-workbench-and-customization-architecture.md)，完成了 Web 活文档工作台的核心后端 API、内嵌高质感交互界面、多格式智能装入器、CLI `rupost web` 指令及 `rupost init spec` 脚手架生成：

### 1. Web 领域抽象与插件化架构 (`src/web/`)
- **开闭接口抽象**：
  - `DocumentStore`：解耦文档文件系统操作，内置 `LocalFsDocumentStore` 并提供严格的 canonical 路径穿越沙箱拦截保护。
  - `AssetProvider`：解耦静态资产托管，同时内置 `EmbeddedAssetProvider`（零外部依赖直接内嵌高审美前端）与 `LocalDirAssetProvider`（支持 `--static-dir` 用户接入自主实现的第三方前端应用，彻底解除 UI 绑定）。
  - `MultiFormatImporter`：支持多格式（cURL, HTTPie, Raw HTTP, HAR, OpenAPI, Postman）智能格式嗅探与转换为规范的 Markdown/HTTP 块，支持 origin 模板化为 `{{base_url}}` 并可选生成 `@mock-default` 兜底变体。
- **核心 RESTful API 契约与防线**：
  - `GET /api/v1/files` & `GET /api/v1/file`：列出及读取工作区活文档。
  - `POST /api/v1/save`：文档落盘保存，严格内嵌**只读保护防线**（403 Forbidden）与 **AST 预写入语法校验门禁**（422 Unprocessable Entity）。
  - `POST /api/v1/run/block`：原位单块调试执行，无缝复用 `TestExecutor` 内存会话与时延瀑布图转换，直接返回细粒度响应与断言结果。
  - `POST /api/v1/mock/simulate`：Mock 变体规则诊断仿真探针，针对传入的变体条件（Header, Query, Body JSONPath）实时求值并输出命中诊断树。
  - `POST /api/v1/import` & `POST /api/v1/import/curl`：统一装入与 cURL 别名接口。
- **现代化内嵌前端**：
  - 零外部 npm/node 依赖，单文件自包含 HTML/CSS/JS，遵循 HSL 调色规范与三栏响应式活文档编辑/原位调试体验，内置代码智能装入向导 Modal。

### 2. CLI `rupost web` 指令集成
- 在 `src/cli.rs` 中新增 `Web` 子命令，支持 `--port` (默认 3000)、`--host` (默认 127.0.0.1)、`--static-dir` (外部静态资源覆盖)、`--readonly` (只读保护模式) 与 `-d, --dir`。
- 严格遵循 Clean Architecture 入口解耦规范与 CLAP Parser Rule，补齐表驱动参数解析单元测试。

### 3. 脚手架生成与全量三层质量验收
- 新增 `SpecTemplate` 策略与内置模板文件（`templates/spec/template.md`, `template.http`, `env.example`），原生支持 `rupost init spec` 生成标准活文档契约规格与双角色用例。
- **三层质量全量回归**：
  1. `cargo test`：522 个单元测试与集成测试（包含新增的 `tests/web_api_integration_test.rs` 与 `tests/template_test.rs`）100% 全部通过。
  2. `tests/verify_features.sh`：全特性集成冒烟测试脚本 100% PASS。
  3. `examples/run_all.sh`：15 个复杂示例回归测试脚本 100% 全部 PASS。
  4. `cargo clippy --all-targets --all-features -- -D warnings` 与 `cargo fmt --all -- --check` 均零告警零错误。

---

## Stage 5（安全基础设施组件化、核心领域解析内聚与 Web 活文档全景加固）落地完成（2026-11-07）

针对架构评审与安全审查提出的解耦复用与防护漏洞，依据 Clean Architecture 与 DRY 原则完成了系统级安全组件化与核心解析内聚升级：

### 1. 统一安全基础设施构建 (`src/security/`)
- **`PathSandbox` 路径安全沙箱 (`src/security/path.rs`)**：
  - 彻底收敛散落各处的路径检查逻辑，统一负责相对路径与绝对路径规范化、扩展名白名单校验（忽略大小写）、隐藏文件拦截（防止读取 `.env`、`.git/` 等敏感配置）。
  - 实现基于祖先追溯的软链接逃逸防御（符号链接穿越阻断）。
  - `LocalFsDocumentStore` 与 `LocalDirAssetProvider` 100% 委托 `PathSandbox`，彻底根除了静态资产服务中的任意文件读取漏洞（VULN-01）。
- **`HtmlSanitizer` 安全转义组件 (`src/security/sanitize.rs`)**：
  - 严格支持 5 种核心 HTML 实体（`& < > " '`）的高性能转义，基于 `Cow<'a, str>` 实现零额外分配。
  - `ReportContext` HTML 报告转义与 Web 前端错误横幅统一对齐。

### 2. 核心领域内聚重构与对称性打通
- **`MockVariantParser` 领域解析组件 (`src/parser/mock_variant.rs`)**：
  - 提炼变体规则切分与解析核心逻辑，`MarkdownFileParser` 删除 120+ 行冗余代码。
  - 在 `HttpFileParser` 中原生接入变体解析，使纯 `.http` 格式完整获得单块双角色与 Mock 变体支持，彻底消除了两种格式的不对称性。
- **`parse_document_by_path` 门面解析分发 (`src/parser/mod.rs`)**：
  - 统一依据扩展名安全分发 Markdown 与 HTTP 文件解析，Web 保存接口 `save.rs` 统一接入作为预写入语法校验门禁。
- **`UrlTemplatizer` 强类型 URL 模板化组件 (`src/http/url.rs`)**：
  - 彻底重构 cURL/HTTPie/RawHTTP 中重复的基准 URL 模板化逻辑。
  - 杜绝基于字节切片的非 ASCII 国际化中文域名（IDN）Panic 风险，妥善保留 Query 与 Fragment。
- **纯 `.http` 变量定义兼容**：
  - 识别并跳过内联/文件级 `@var = value` 变量声明，防止误判为非法 HTTP 请求行导致 422 语法报错。

### 3. Web 活文档端到端加固与契约修复
- **Host Guard 门禁**：
  - 在 Axum Web 服务器挂载 Host 检查中间件，严格白名单过滤（`localhost`, `127.0.0.1`, `::1`, `*.localhost`），彻底防御 DNS Rebinding 攻击。
- **请求体积限制**：挂载 `DefaultBodyLimit::max(10 * 1024 * 1024)`（10MB）。
- **前端客户端修复**：
  - 修复 `app.js` 中 `openFile` 对 `json.data` 取值兼容性 Bug（解决编辑器显示 `"undefined"` 的 P0 体验问题）。
  - 修复前端 `escapeHtml` 实体转义漏洞与错误横幅未转义渲染导致的 DOM XSS 隐患。

### 4. 三层全量质量回归与自动化验证
- **单元与集成测试**：538 项测试（含新增的 Host 守卫、Path 沙箱阻断、`.http` 语法门禁、IDN 中文域名装入等对抗性测试）100% PASS。
- **全特性冒烟测试**：`tests/verify_features.sh` 脚本端到端验证 100% PASS。
- **示例场景全量回归**：`examples/run_all.sh` 15 个场景全部 PASS。
- **代码规范**：`cargo clippy --all-targets --all-features -- -D warnings` 与 `cargo fmt --all -- --check` 零告警零错误。

---

## 7 轮对话阶段性归档与待执行任务清单 (2026-11-07 19:10)

本阶段完成了核心领域组件化先行方案落地、双子代理对抗式 Code Review（Blue Team 架构专家 + Red Team 安全审计专家）以及网络层 Default-Deny 原理审查。全量自动化测试基线维持在 538 tests 100% PASS。

### 待执行任务全景清单 (按严重性与落地优先级分级)

#### 🚨 第一梯队：高危安全防线加固 (P0/P1)
1. **【SEC-01】Host 校验缺失拦截与 Default-Deny 闭环 (`src/web/server.rs`)**：
   - 贯彻 RFC 9112 契约，缺少 `Host` 标头或格式非法时，直接拒绝返回 `400 Bad Request`，杜绝 HTTP/1.0 绕过。
2. **【SEC-02】`allowed_hosts` 运行时配置接线与 CLI 扩展 (`src/web/server.rs`, `src/cli.rs`)**：
   - 中间件注入 `State(state)`，使用户的 `--allowed-hosts` 真正参与白名单校验，防止内网合法访问被误杀。
3. **【SEC-03】`PathSandbox` 悬空软链接逃逸阻断 (`src/security/path.rs`)**：
   - 使用 `candidate.symlink_metadata().is_ok()` 替代 `candidate.exists()`，严防指向沙箱外不存在目标的软链接穿透写入。
4. **【SEC-04】只读模式（`--readonly`）单块发包拦截 (`src/web/handlers/run.rs`)**：
   - 在 `run_block_handler` 增加 `state.readonly` 检查（返回 403），防御内网 SSRF 嗅探探测。
5. **【SEC-05】外部静态资产目录扩展名白名单 (`src/web/assets.rs`)**：
   - 为 `LocalDirAssetProvider` 沙箱配置合法 Web 资产白名单（`.html`, `.css`, `.js`, `.svg`, `.png`, `.json` 等），防止误挂载导致源码泄露。

#### ⚠️ 第二梯队：Clean Architecture 解耦与健壮性 (P2)
6. **【ARCH-01】解除 `MockVariantParser` 与 `HttpFileParser` 的循环依赖 (`src/parser/`)**：
   - 提取通用的 Header 词法解析工具函数，阻断 `mock_variant` 对 `http_file` 的反向引用。
7. **【ARCH-02】单块执行自适应复用 `parse_document_by_path` (`src/web/handlers/run.rs`)**：
   - 消除硬编码包裹 Markdown 栅栏的迂回逻辑，对纯 `.http` 原生解析。
8. **【ARCH-03】安全领域错误强类型化 (`src/error.rs`, `src/security/path.rs`)**：
   - 引入 `SecurityViolation` 强类型枚举变体，使安全拦截精准映射为 HTTP 403。
9. **【ROBUST-01】Mock 变体切分器增加 Body 边界上下文感知 (`src/parser/mock_variant.rs`)**：
   - 防止 POST 请求体 JSON 内含有 `@mock-when` 字符串时被破坏截断。
10. **【ROBUST-02】`PathSandbox` 前置纯词法 `..` 判定 (`src/security/path.rs`)**：
    - 词法阶段拦截目录穿越，减少不必要的物理磁盘探测。
11. **【UX-01】前端界面对 `--readonly` 状态的感知与置灰 (`src/web/assets/app.js`, `index.html`)**：
    - 顶部展示 `[Read-Only]` 徽章，置灰保存与执行按钮。

#### 💡 第三梯队：异步性能与代码微调 (P3)
12. **【PERF-01】`LocalFsDocumentStore` 同步 I/O 异步化 (`src/web/store.rs`)**：逐步由 `std::fs` 迁移为 `tokio::fs`。
13. **【PERF-02】单块发包热路径配置缓存 (`src/web/handlers/run.rs`)**：缓存 `ConfigLoader` 结果。
14. **【ROBUST-03】IPv6 独立主机识别增强 (`src/http/url.rs`)**。
15. **【UX-02】前端调试面板 `data.status` 防御性转义 (`src/web/assets/app.js`)**。

---

## 查漏补缺与加固综合执行方案归档 (2026-11-07 21:35)

已将完整的产品功能查漏补缺（包含 `run/suite`、多格式 HAR/Postman 真实解析、`--dev` 与 CORS 跨域支持、前端一键整套测试交互）与红蓝审查加固方案完整落地保存至本地文件：
- 活跃规划文档：[doc/plans/active/2026-11-07-web-workbench-missing-features-and-hardening-plan.md](file:///Users/zsyzzx/project/rust/rupost/.workspace/role-alignment-scenario/doc/plans/active/2026-11-07-web-workbench-missing-features-and-hardening-plan.md)
- 包含详尽的数据模型、架构分层、安全防线加固算法以及 5 组生产级核心示例代码。







---

## Web 安全防线与基础功能批次交付 + CLI 安全评估归档（2026-11-07 深夜）

依据用户裁决执行顺序（安全 → 基础功能 → CLI 评估 → 导入 PRD 归档），7 个原子提交 + 2 份文档交付：

### 1. 批次 1：Web 安全防线（4 提交，TDD 对抗测试同提交）
- **SEC-01/02**：host_guard 中间件 Default-Deny 闭环——缺失/非法编码 Host 标头返回 400（RFC 9112，杜绝 HTTP/1.0 绕过），`is_allowed_host` 接入 `state.allowed_hosts` 动态白名单；CLI `rupost web` 新增 `--allowed-hosts`（逗号分隔多值）。
- **SEC-03**：PathSandbox 以 `symlink_metadata` 判存在——悬空软链接（旧实现 exists() 判 false 走"新建文件"分支放行，写入跟随链接逃逸沙箱外）一律拒绝。
- **SEC-04**：run_block_handler 只读模式 403（与 save 对称，SSRF 内网嗅探防线闭环）。
- **SEC-05**：LocalDirAssetProvider 扩展名白名单（html/css/js/mjs/json/svg/png/jpg/jpeg/ico/woff/woff2），防 --static-dir 误挂载外泄源码。
- 回归：批次收尾 `cargo test --workspace` **860 passed / 0 failed**，clippy/fmt 双零。

### 2. 批次 2：基础功能完善（6 提交）
- **ARCH-03**：`RupostError::SecurityViolation` 强类型变体 + save handler 精准映射 403（原 400 契约随测试更新）。
- **ARCH-01**：提取 `parser::parse_http_header` 公共函数，解除 mock_variant ↔ http_file 模块内循环引用。
- **ARCH-02**：run_block 复用 `parse_document_by_path` 门面分发，纯 .http 原生解析（消除硬编码 Markdown 栅栏迂回）。
- **ROBUST-01**：mock 变体切分器 Body 边界上下文感知——请求体内以 `@mock-when` 开头的内容行不再误判截断（对抗测试锁定）。
- **ROBUST-02**：PathSandbox 词法 `..` 前置拦截（零磁盘探测，两分支统一）。
- **UX-01**：`GET /api/v1/meta` 端点 + 前端只读徽章与写路径按钮置灰。
- **批次 2b**：`POST /api/v1/run/suite` 整文档批量执行端点（readonly 403 防线自带 + wiremock 真实三块套件聚合断言 + 404/422 错误形态）。
- 回归：**866 passed / 0 failed**，clippy/fmt 双零。

### 3. 批次 3：CLI 安全评估（只读 + 隔离 PoC 实证，报告归档）
- 报告：[doc/dev/2026-11-07-cli-security-assessment.md](doc/dev/2026-11-07-cli-security-assessment.md)。
- **P1-1（实证）**：`--report-file` 静默覆盖已存在文件且跟随符号链接写入任意用户可写目标——修复建议与"分支收尾批次"报告覆盖提示同源合并。
- **P2-1（实证但不可执行注入）**：import/generate 生成物结构污染——body 中 `###`/指令样文本行原样写入；三类走私（header 换行 / 裸元数据 / 谓词拆分）均被既有防线阻断。
- 其余面板（钩子/沙箱/泄露/Cookie）防线充分；建议 `make audit`（cargo audit）纳入发版仪式。

### 4. 导入功能：PRD 归档（不实施，用户裁决后移 + CLI 结合）
- [doc/plans/active/2026-11-07-importer-unification-prd.md](doc/plans/active/2026-11-07-importer-unification-prd.md)：HAR/Postman/OpenAPI 真实解析落位 `src/importer/` 领域层，CLI `import --format` + Web `/api/v1/import` 双端同源；R1-R6 需求 + 6 提交切分 + 勘误 `:376` 失实宣称的联动登记。
---

## 阶段执行进度：第一步收口微调与第二步 1（通用 Importer 领域模块提取）已达成（2026-11-07 深夜）

依据最新合并路线图与用户指令“先完成第一步，和第二步的 1”，完成以下两大原子演进与提交：

### 1. 第一步收口微调 (Commit `kzlvpxwu` a10e65ed)
- **RFC 9110 §7.2 Host 标头大小写无关**：`src/web/server.rs` 中的 `is_allowed_host` 改用 `eq_ignore_ascii_case` 匹配 `localhost` 与 `state.allowed_hosts` 动态白名单，兼容 `Host: Localhost:3000`、`LOCALHOST` 等合规大小写混合请求。
- **空文档 422 语法门禁统一**：`src/web/handlers/run.rs` 在解析文档后校验 `parsed_file.requests.is_empty()`，若为空（包含空 `.http` 或无 ```` ```http ```` 块的纯文本 Markdown）统一返回 422 `UNPROCESSABLE_ENTITY`（`"Document contains no executable HTTP request blocks"`）。
- **TDD 校验**：`tests/web_api_integration_test.rs` 新增混合大小写 Host 访问（200 OK）与无请求 Markdown 套件执行（422）断言。

### 2. 第二步 Item 1：通用 Importer 领域模块提取 (Commit `spwmoszx` 608c4a47)
- **Clean Architecture 领域下沉**：
  - 新建/扩展独立领域模块 `src/importer/`，将原混杂在 `src/web/` 中的请求解析与协议转换逻辑彻底移入核心领域层，解除 Web / Axum 依赖。
  - `src/importer/mod.rs`：定义 `RequestImporter` 契约 Trait、`ImportFormat`、`ImportRequest`、`ImportResponse` 与 `MultiFormatImporter` 门面调度器。
  - `src/importer/curl.rs`：实现 `CurlImporter: RequestImporter`，复用既有 POSIX 分词与请求合成能力。
  - `src/importer/httpie.rs`：实现 `HttpieImporter: RequestImporter`，支持简写 URL、参数展开、JSON 合成与 Host 模板化。
  - `src/importer/raw.rs`：实现 `RawHttpImporter: RequestImporter`，遵循 RFC 7230 解析标准 Raw HTTP 报文。
- **Web 适配层向后兼容**：
  - `src/web/importer.rs` 与 `src/web/types.rs` 透明重导出 `crate::importer::*`，上层路由适配器零破坏。
- **全量回归验证**：
  - `cargo test --lib importer`：28 个单元测试全绿；
  - `cargo test --test import_curl_test`：5 个端到端集成测试全绿；
  - `cargo test --test web_api_integration_test`：14 个 Web API 集成测试全绿；
  - `cargo test --lib`：全量 540 个单元测试 100% 通过；
  - `cargo clippy --all-targets --all-features -- -D warnings` 与 `cargo fmt --all -- --check` 保持双零告警。


---

---

## 分支收尾批次（branch-cleanup bookmark，6 原子提交，2026-11-08）

今晨审查 + 双子代理对抗审查（设计 0 P0 / 测试策略 3 P0）修订后的方案全部交付：

1. **Json\* 报告契约迁入 `src/report/json.rs`**（提交 1）：reporter.rs 收敛为纯终端渲染器，report→reporter 反向依赖消除；瀑布换算唯一化——`from_diagnostics`（残差 clamp）/ timing 分支保全 / `from_total_ms(f64)` 三分支各守其形，双份 ~40 行重复删除；新增 serde(default) 旧版最小报告反向兼容负向测试。
2. **`run_test_suite` 返回 `TestSuiteOutcome`**（提交 2，破坏性 API）：失败判定口径单点维护；4 调用点全适配（编译器当检查器，不加 From<bool> 兼容）；新增子进程 e2e 锁定失败 exit 1 / 全绿 exit 0（RUPOST_HISTORY_DIR 隔离）。
3. **报告文件输出上移表现层**（提交 3）：`write_report_files` 门面返回 `ReportWriteOutcome` 消息结构（打印决策留派发层，stdout 纯净可测）；格式判定决策表 11 行表驱动——显式 `--report` 永远优先 + 大小写统一（`--report json --report-file x.html` 行为变更入 CHANGELOG）；覆盖提示 stderr 数据化（评估 P1-1 闭环：允许覆盖 + 提示 + symlink 行为文档测试）；默认路径子进程测试（禁 set_current_dir）。
4. **scenario 测试基建去 flake**（提交 4）：7 处固定端口（19188-19194/19198）+ 吞 bind 错误 + sleep 竞态 → `bind(":0")` + `serve_matcher` 直传已绑定 listener，bind expect 在 spawn 外；mock_integration_test 存量登记不顺手修。
5. **MockCompiler 死代码删除**（提交 5）：guard 后不可达兜底分支 + 注释修正（双角色语义 CHANGELOG 登记）。
6. **文档联动**（提交 6）：directory_structure.md 登记 `src/report/` 五文件 + `BatchResult` 幽灵类型修正 + reporter 条目更新；progress_summary Sprint 6 状态与 main.rs 行数勘误；CHANGELOG Changed/Fixed。

---

## 解耦提案双子代理审查与 v2 修订（2026-11-08）

[2026-11-08-codebase-decoupling-and-monolith-refactoring-proposal.md](doc/plans/active/2026-11-08-codebase-decoupling-and-monolith-refactoring-proposal.md) 经架构审查 + 事实核查双子代理对抗审查（无 P0 架构错误，1 项 P0 执行卡壳点 + 6 项 P1 全部吸收），文档重写为 v2：

1. **P0 勘误**：靶点三组件树按真实 `DetailTab`（Response/Headers/Request/Results/Diagnose）重画——原"Overview/Timing/Body"清单系沿用本文件旧条目过时表述（已随批勘误），Timing 实内联于 Response 臂。
2. **设计缺口补齐**：渲染期副作用（预折行缓存惰性重建）归属显式化——门面 `&mut` 前置重建、子组件 `&AppState` 只读；editor.rs 552 行测试块与 219 行 `trigger_file_execution` 最大块归属声明（迁 `actions/execution.rs` + 测试随迁）；靶点四改写为"内联字段提取 Args 结构体"（原"平铺结构体拆分"失实——实为 enum 内联字段），12 变体全覆盖 + dispatch/runner 归位。
3. **验证纪律补 bin 防线**：cli 属 bin crate，12 个单测（含位置契约表）`cargo test --lib` 跑不到——§5 补 `cargo test --bin rupost`，迁移前先扩契约表（别名/value_delimiter/trailing_var_arg 保真清单）。
4. **指标校准**：AppState 69 字段（非 46）；state/mod.rs 目标 670→~290（降 57%，非 75%）；editor 四块实测 57/44/44/80 行；高亮 3 函数 6 测子串匹配（非 4 组 11 测正则）。
5. **路线图改 1 → (4 ∥ 2) → 3**（cli/tui 零依赖可并行，response 殿后）；范围外五巨石（html.rs 897 等）登记 future_backlog。
