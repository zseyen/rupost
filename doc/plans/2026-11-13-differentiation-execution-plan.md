# RuPost 差异化路线执行计划

**日期：** 2026-11-13  
**状态：** Ready for implementation  
**前置决策：** [竞品与差异化开发报告](2026-11-13-competitive-differentiation-report.md)

## 1. 目标、范围与完成定义

本计划只交付两件事：

1. 修复 Web Workbench 的跨浏览器会话泄漏，并让发布验证可靠、非破坏性。
2. 交付“故障复现包”MVP：一次失败的 `rupost test` 能产出脱敏、可重放、可被 CI 或同事消费的目录工件。

完成后，用户应能运行：

```bash
rupost test docs/payments.md --env staging \
  --incident-out artifacts/payment-failure \
  --mask-secret-value "$PAYMENT_TOKEN"

rupost replay artifacts/payment-failure/snapshot.json --target http://localhost:8080
rupost ai triage artifacts/payment-failure/snapshot.json
```

`--incident-out` 仅在运行失败时生成工件；成功运行不创建目录。工件始终开启键名和显式值脱敏，绝不提供关闭脱敏的开关。

### 非目标

- 不做云同步、账号、团队工作区、远程工件上传或定时监控。
- 不引入新的脚本语言、AI 执行期决策或 k6 类压测调度器。
- 不重写现有 `ExecutionSnapshotFile`、JSON 报告、回放器或脱敏引擎。
- 不让 Web Workbench 与 CLI 的执行语义在本批次完全合流；本批只修正隔离边界。

### 总体验收

| 编号 | 验收事实 |
| --- | --- |
| A1 | 两个 Web 浏览器会话不能互读 Cookie 或 `@capture` 变量。 |
| A2 | `tests/verify_features.sh` 不会终止用户已有 Rupost 进程，且任何被测套件失败都会让脚本失败。 |
| A3 | 失败运行产生 `manifest.json`、`report.json`、`snapshot.json`；文件没有原始凭据。 |
| A4 | 复现包内 `snapshot.json` 可被已有 `rupost replay` 直接消费。 |
| A5 | `cargo fmt --all -- --check`、Clippy、全量测试、Web 前端测试、特性脚本与 examples 回归均通过。 |

## 2. 固定架构决策

### 2.1 Web 会话模型

在 `src/web/session.rs` 新建 Web 适配层专用的 `WebSessionStore`；不要把会话模型放进 `runner`、`variable` 或 HTTP 领域层。

```text
浏览器请求
  -> Cookie: rupost_session=<opaque UUID>
  -> WebSessionStore::load_or_create
  -> WebSession { cookie_state, captured_variables, last_access }
  -> run_block / run_suite
  -> WebSessionStore::persist
  -> Set-Cookie（首次创建时）
```

约束：

- Session ID 使用现有 `uuid` 依赖生成，不接受调用方 JSON 传入的身份标识。
- Cookie 名为 `rupost_session`，属性固定为 `Path=/api/v1; HttpOnly; SameSite=Strict; Max-Age=1800`。不指定 Domain，保持 host-only；本地 HTTP 模式不能加 `Secure`。
- 存储只在进程内保存，TTL 为 30 分钟，最多 128 个活跃会话；每次 load/create 清理过期项，再按最久未访问项淘汰。
- `AppState` 只持有 `Arc<WebSessionStore>`，不再有全局 `cookie_bridge` 和 `run_variables`。
- 绝不跨 `.await` 持有 `std::sync::Mutex`。load 返回本次执行所需的克隆快照，执行完成后只持锁合并结果。
- 未携带 Cookie、Cookie 不存在、格式非法或已过期都等价于新会话；不向客户端泄露内部失败原因。

选择服务端存储而非把 Cookie/变量放进浏览器：浏览器绝不接触上游认证 Cookie 或捕获的 secret，且工作台现有请求体不用扩展 session 字段。

### 2.2 故障复现包格式

在 `src/history/incident.rs` 定义唯一的 `IncidentManifest` 和 `create_incident_bundle`。它是历史/工件层，不得依赖 `cli`、TUI 或 Web。

目录固定为：

```text
<incident-out>/
  manifest.json       # Incident schema 1.0，文件名、时间、来源、计数、脱敏声明
  report.json         # 复用 JsonFileReport 契约
  snapshot.json       # 复用 ExecutionSnapshotFile Schema v1.0，强制 masking=secrets
```

`manifest.json` 必含：`schema_version`、`created_at`、`source_paths`、`environment`、`total`、`failed`、`skipped`、`artifacts`、`masking`、`max_snapshot_body_bytes`。不写入变量值、Cookie、插件参数、绝对工作目录或 AI 输出。

输出路径必须不存在；若已存在、是符号链接或无法建立私有目录，返回 `SecurityViolation`。在父目录创建受控临时目录，三个文件全部写完后才 rename 到最终目录；失败时删除该临时目录。Unix 下目录为 `0700`、文件为 `0600`。复用 `security::fs` 的原子私有写入器，新增最小的“创建私有目录/临时目录”能力，不能另写一套权限逻辑。

## 3. 工作包与原子提交

每个工作包按 Red -> Green -> Refactor 完成。开始前运行 `jj status`；只在工作副本包含本工作包文件时提交。提交后运行 `jj diff -r @- --stat`，确认没有吸收其他改动。

### WP-0：建立失败守护测试

**目的：** 在改生产代码前锁定隔离、工件与脚本可信度。

**文件：**

- 修改 `tests/web_session_reuse_test.rs`
- 新增 `tests/incident_bundle_test.rs`
- 修改或新建适合 shell 的测试辅助，不复制现有 Rust 集成测试

**先写测试：**

1. 改造 `run_block` helper，使其可接收 cookie，并从响应提取首个 `Set-Cookie` 中的 `rupost_session` 值。
2. 新增 `test_web_session_does_not_share_captured_variables_between_clients`：
   - Client A 登录，服务端返回 `tok-client-a`；保存 A 的会话 cookie。
   - Client B 不携带 A cookie，带 `Authorization: Bearer {{auth_token}}` 请求必须得到 401。
   - Client A 使用其 cookie 调用同一请求必须得到 200。
   - 临时删除生产代码的 session lookup/隔离逻辑时，此测试必须稳定变红。
3. 新增复现包 CLI 集成测试：wiremock 返回含 Bearer token 的失败响应；运行二进制 `test ... --incident-out <不存在目录> --mask-secret-value <token>`，断言退出码为 1、三文件存在、`manifest.json` 的契约字段精确匹配、任一文件不含原始 token，并能用 `replay snapshot.json --target <wiremock>` 运行。
4. 新增成功对照：`--incident-out` 的全绿运行不创建目标目录。
5. 新增输出路径保护：预先创建目标目录和符号链接（Unix）后，命令必须失败且不得修改其中的 sentinel 文件。

**验证：** 新测试在未实现前失败，保留失败记录；不提交红色测试。

### WP-1：Web 会话隔离

**目的：** 解决 P0 跨客户端认证态与变量泄漏。

**生产文件：**

- 新增 `src/web/session.rs`
- 修改 `src/web/mod.rs` 导出内部模块
- 修改 `src/web/server.rs`
- 修改 `src/web/handlers/run.rs`
- 修改 `tests/web_session_reuse_test.rs`

**实施步骤：**

1. 在 `session.rs` 写纯数据结构 `WebSession` 与 `WebSessionStore`，包含 `cookie_state: Option<serde_json::Value>`、`captured_variables: HashMap<String, String>`、`last_access`。时钟应以 `Instant` 计算 TTL，避免系统时间跳变。
2. 让 `WebSessionStore::load_or_create(&HeaderMap)` 返回 `{ id, state, set_cookie }`；严格解析 `Cookie` 请求头中的单个 `rupost_session` 值，其他 cookie 忽略。
3. 用 `WebSessionStore::persist` 接收执行后的 cookie bridge 与变量增量；跳过 `__` 前缀变量，保留当前“显式 `extra_variables` 优先”的规则。
4. 将 `AppState.cookie_bridge` / `run_variables` 改为一个 `sessions` 字段；在 `create_web_router` 初始化 store。
5. 在 `run_block_handler` 与 `run_suite_handler` 提炼共享的三个私有 helper：装载会话到 `VariableContext`、导入 executor cookie、持久化执行后的增量。两个 handler 禁止各自复制一份 session 逻辑。
6. 首次创建的成功和业务失败响应都必须附带 `Set-Cookie`；解析错误、只读拒绝等执行前失败不创建 session。
7. 保持 API body schema 不变。Cookie 是唯一新增 transport 契约。

**必须避免：**

- 不把 UUID 放进 `RunBlockRequest` 或 `RunSuiteRequest`。
- 不把 session 状态写入磁盘、历史记录、报告或日志。
- 不允许 `--dev` 绕过隔离；宽松 CORS 不是共享身份的理由。
- 不改变 CLI/TUI Cookie 行为。

**测试和提交：**

```bash
cargo test --test web_session_reuse_test
cargo test --test web_api_integration_test
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
jj commit -m "fix(web): isolate per-client execution sessions"
```

### WP-2：特性脚本与 CI 可信度

**目的：** 发布前验证不能产生假绿或干扰开发者进程。

**文件：**

- 修改 `tests/verify_features.sh`
- 修改 `.github/workflows/ci.yml`
- 必要时修改 `doc/guides/testing_guide.md`

**实施步骤：**

1. 删除 `killall rupost`、固定 `./tests_temp_dir` 和对仓库根 `rupost.toml`/`.env` 的移动覆盖。
2. 以 `mktemp -d` 创建工作区，在脚本开头解析 `RUPOST_BIN` 为绝对路径后立即 `cd` 到该工作区。所有输入、报告、临时配置都只在该目录或其显式 `/tmp` 子项。
3. 维护 `PIDS` 数组与 `cleanup()`，使用 `trap cleanup EXIT INT TERM`；只 kill 本脚本启动且仍存活的 PID；cleanup 必须幂等。
4. 替换固定端口 9000--9004：每个 Mock 端口读取可覆盖的 `RUPOST_TEST_PORT_BASE` 并加偏移；启动后使用有限重试的健康探针，而不是固定 `sleep 2`。端口冲突必须给出可读错误，不得杀其他进程。
5. 删除 `rupost test ... || true`。捕获退出码并明确断言为 0；同时断言代表业务成功的响应/摘要，不以出现文件名作为成功条件。
6. 所有“预期失败”命令使用 `EXIT=0; command || EXIT=$?` 并精确比较预期退出码。
7. 在 CI 增加独立 Ubuntu `web-assets` job：`actions/setup-node@v4` 后执行 `node --test tests/web/app_test.mjs`。不要放到 Windows matrix 的 `make` 路径。

**验证：**

```bash
bash -n tests/verify_features.sh
./tests/verify_features.sh
make test-web
```

手工守护：先启动一个独立 `rupost mock`，再运行脚本；脚本结束后该独立进程仍必须存活。临时让 DAG 请求断言失败，脚本必须返回非零。

**提交：**

```bash
jj commit -m "test: make feature verification isolated and fail-closed"
```

### WP-3：复现包领域工件

**目的：** 复用已有快照和报告，形成一个安全、可移交的失败工件。

**生产文件：**

- 新增 `src/history/incident.rs`
- 修改 `src/history/mod.rs`
- 修改 `src/security/fs.rs`
- 新增或修改 `tests/incident_bundle_test.rs`

**实现顺序：**

1. 在 `incident.rs` 先定义 `INCIDENT_SCHEMA_VERSION`、`IncidentManifest`、`IncidentArtifactNames` 和纯函数 `failure_counts`；使用表驱动单测锁定 schema 字段与成功/失败/跳过计数。
2. 实现 `create_incident_bundle`。输入只接收已产生的 `batch_results`、`resolved_paths`、`environment`、显式 secret values、body 上限和目标路径；不得重新执行请求。
3. 调用已有 `save_batch_snapshot(..., true, secret_values, ...)` 强制产生掩码快照；调用 `report::json::report_json` 写 `report.json`；最后写 manifest。
4. 在 `security::fs` 添加小而专用的私有目录发布原语，确保“不覆盖、拒绝 symlink、临时目录失败清理、最终 rename”。不要让 `incident.rs` 直接散落 `std::fs` 权限判断。
5. `history/mod.rs` 仅重导出所需的工件创建 API；没有 `cli` 依赖。

**关键边界：**

- 复现包内包含整套非 teardown 交互，因为登录等依赖是回放的必要上下文；不只摘失败请求。
- 复现包不复制原始 Markdown/`.http` 文件，避免将用户文档中的字面凭据再次写出。
- snapshot 的既有 `body_truncated`、`masking` 和 replay 降级语义必须保留。
- 目录任一文件写入失败时，最终目标不存在且无临时残留。

**验证和提交：**

```bash
cargo test --lib history::incident
cargo test --test incident_bundle_test
cargo test --test snapshot_contract_test
jj commit -m "feat(history): create redacted incident bundles"
```

### WP-4：CLI 接线与用户契约

**目的：** 用一条熟悉的 `test` 命令生成复现包，不另造第二套执行流。

**生产文件：**

- 修改 `src/cli/commands/test.rs`
- 修改 `src/cli/dispatch.rs`
- 修改 `src/runner/app.rs`
- 修改 `src/cli/suite_presenter.rs`
- 修改相关 CLI 与集成测试

**实施步骤：**

1. 在 `TestArgs` 增加 `--incident-out <DIR>`；帮助文本必须说明“仅失败时创建、目录必须不存在、强制脱敏”。
2. 在 `TestSuiteOptions` 增加 `incident_out: Option<String>`，`Default` 为 `None`；更新所有 struct literal。让编译器列出遗漏，不加兼容 shim。
3. `dispatch` 只映射参数。`runner::execute_test_suite` 仍是纯执行/数据产出，不写任何工件。
4. 在 `present_suite_outcome` 中，在报告与普通快照呈现后判断 `outcome.has_failure && incident_out.is_some()`，调用 `history::create_incident_bundle`。成功时不写目录，向 stderr 输出一条说明。
5. 若工件创建失败，返回错误并使命令退出 2；原测试失败仍保持退出 1。不要用工件 I/O 故障掩盖执行结果。
6. `--incident-out` 与 `--save-snapshot` 可以同时使用，但它们是独立输出；前者永远脱敏且不覆盖，后者维持既有用户选择。

**CLI 验收表：**

| 命令结果 | `--incident-out` 路径 | 进程退出码 | 目录 |
| --- | --- | --- | --- |
| 全绿 | 不存在 | 0 | 不创建 |
| 断言/请求失败，写包成功 | 不存在 | 1 | 创建，含三文件 |
| 断言/请求失败，目标已存在 | 存在 | 2 | 原目录不变 |
| 全绿，目标已存在 | 存在 | 0 | 原目录不变 |

**验证和提交：**

```bash
cargo test --bin rupost
cargo test --test incident_bundle_test
cargo test --test run_exit_code_test
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
jj commit -m "feat(cli): emit incident bundles for failed test suites"
```

### WP-5：文档、示例与全量门禁

**文件：**

- 修改 `README.md`
- 修改 `doc/guides/debug_guide.md`
- 修改 `doc/guides/testing_guide.md`
- 新增 `examples/incident_bundle/README.md` 和本地可运行 fixture
- 修改 `doc/plans/progress_summary.md`
- 修改 `CHANGELOG.md`

**内容：**

- README 只增加一段最小示例，不重复完整 schema。
- 调试指南说明工件生命周期、目录三文件含义、`replay` 与 `ai triage` 消费方式，以及掩码后 body diff 结论受限。
- 测试指南登记新增集成测试、Web 双会话守护和 CI Web job。
- 示例必须使用 loopback Mock，不能依赖公网或真实凭据。
- 最后才更新 `progress_summary.md`，记录已完成事实、测试命令与后续 P2（文档块稳定 ID/契约漂移）；不可把未完成 Phase 2/3 写成已实现。

**最终门禁：**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
make test-web
./tests/verify_features.sh
./examples/run_all.sh
git diff --check
```

若外网 examples 因网络不可达失败，记录失败命令、目标和错误；不可将其标记为通过，也不可为“绿灯”删除外网验证。

提交：

```bash
jj commit -m "docs: document incident bundle workflow"
```

## 4. 接手说明（给 flash 级别模型）

1. 一次只做一个 WP。开始前读本文件对应章节、目标源文件和目标测试；不要顺手重构相邻模块。
2. 先改测试，再实现最小生产代码。每个新守护测试都应通过暂时破坏实现来证明会变红，然后恢复。
3. 保持依赖方向：`history/incident` 可以依赖领域数据、报告 JSON 和安全原语；它不能依赖 `cli`、`web`、`tui`。CLI 只解析和呈现。
4. 所有相对展示路径继续走 `normalize_posix_path`；所有工件写入继续走 `security::fs` 的权威安全路径。
5. 快照和报告 schema 是外部契约：只追加字段，不改字段语义；本批不改 `ExecutionSnapshotFile` schema。
6. 任何涉及 Cookie、变量、快照、报告、URL 或错误文本的新路径都要经现有 redactor；默认安全失败优于静默泄漏。
7. 使用 `rg` 搜索 `TestSuiteOptions {`、`save_batch_snapshot(`、`cookie_bridge` 和 `run_variables`，让编译器作为迁移清单；迁移完成后旧名搜索应为零。
8. 提交前只提交本 WP 文件。执行 `jj diff -r @- --stat` 后再进入下一 WP。

## 5. 交付顺序与检查点

| 检查点 | 可交付内容 | 禁止进入下一步的条件 |
| --- | --- | --- |
| C0 | WP-0 红测设计完成 | 没有跨客户端泄漏的反例测试 |
| C1 | Web 会话隔离 | 任一客户端仍可复用另一客户端 token/Cookie |
| C2 | 验证脚本与 CI 修复 | 脚本仍有 `killall rupost`、`|| true` 掩盖成功路径，或 Web 测试未进入 CI |
| C3 | 领域复现包 | 工件可含原始 secret、可覆盖目标或不能被 replay 消费 |
| C4 | CLI 接线 | CLI 再实现一套快照/报告/脱敏逻辑 |
| C5 | 文档与全量门禁 | 有代码未文档化，或任一必跑门禁未报告结果 |

完成 C5 后，再讨论 Phase 2 的 Markdown 稳定请求 ID 与契约漂移。不要在本计划实施期间提前做 OpenAPI、k6、脚本引擎或云服务。
