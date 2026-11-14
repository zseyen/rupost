# RuPost 测试与功能验证指引 (Testing & Verification Guide)

本文档面向测试工程师与核心开发者，详细梳理了 RuPost 的测试架构矩阵、基于 `examples/` 示例的 E2E 手工验证步骤，以及如何执行代码测试与覆盖率分析。

> **父纲**：全局测试策略、功能覆盖矩阵与缺口行动计划见 [全量测试方案](file:///Users/zsyzzx/project/rust/rupost/doc/plans/2026-10-25-test-strategy-plan.md)，本文档是其 L5 手工验证层的操作手册。

---

## 1. 自动化测试架构矩阵

RuPost 维持了极高的测试覆盖质量，所有的核心业务逻辑和网络适配器均有完备的自动化集成测试。以下是主要的测试用例分布：

| 测试用例文件 | 覆盖的特性与核心功能 | 验证指标 |
| :--- | :--- | :--- |
| [`user_agent_test.rs`](file:///Users/zsyzzx/project/rust/rupost/tests/user_agent_test.rs) | User-Agent 级联覆盖 | 验证：默认 UA -> 变量配置覆盖 -> 用例 Header 最高优先级覆盖的级联链路。 |
| [`cookie_test.rs`](file:///Users/zsyzzx/project/rust/rupost/tests/cookie_test.rs)<br>[`cookie_isolation_test.rs`](file:///Users/zsyzzx/project/rust/rupost/tests/cookie_isolation_test.rs) | Cookie 管理与并行隔离 | 验证：Cookie 自动保存与提取，多并行分支测试时的 Cookie 物理隔离。 |
| [`variable_integration_test.rs`](file:///Users/zsyzzx/project/rust/rupost/tests/variable_integration_test.rs) | 四层级变量覆盖逻辑 | 验证：CLI、环境变量、本地 `.env` 及 `rupost.toml` 的级联覆盖与优先级。 |
| [`batch_testing_test.rs`](file:///Users/zsyzzx/project/rust/rupost/tests/batch_testing_test.rs) | 拓扑 DAG、并行调度、沙箱防护及状态单向克隆 | 验证：Kahn 算法拓扑排序、有向依赖环检测、并行模式下的并发限制、Fail-Fast 流程中断，以及有依赖节点间 `State Cloning`。<br>包含对路径穿越越权拦截 `test_dependency_resolver_sandbox_jail` 的测试。 |
| [`diagnose_integration_test.rs`](file:///Users/zsyzzx/project/rust/rupost/tests/diagnose_integration_test.rs)<br>[`timing_diagnostics_test.rs`](file:///Users/zsyzzx/project/rust/rupost/tests/timing_diagnostics_test.rs) | 连通性分析与 X.509 解析 | 验证：时延时序拆分的计算合理性、证书 Validity 的剩余天数计算、临期/过期判定。 |
| [`mock_integration_test.rs`](file:///Users/zsyzzx/project/rust/rupost/tests/mock_integration_test.rs) | Trie 树匹配与变体评估 | 验证：路径参数捕获（`:id`）、通配符匹配、Header/Query/Body JSONPath 变体匹配及兜底逻辑。 |

---

## 2. 基于内置示例的手工功能验证大纲 (E2E Manual Testing Checklist)

为了能够零门槛、免碎文件干扰地手动验证 RuPost 的核心能力，你可以直接使用 codebase 中内置在 `examples/` 下的各类 HTTP/Markdown 演示用例。

### 2.1 验证 1：模板配置快速初始化 (rupost init)
1.  **准备环境**：新建一个没有任何配置的空文件夹，如 `mkdir /tmp/rupost_test && cd /tmp/rupost_test`。
2.  **执行命令**：
    ```bash
    cargo run --bin rupost -- init
    ```
    *   **预期输出**：控制台输出绿色高亮 `[✓] 成功在当前目录初始化默认 rupost.toml 模板！`。
    *   **结果校验**：检查当前目录下已生成标准的 `rupost.toml` 配置文件，且包含开发、测试、生产等多套配置。
3.  **防覆盖校验**：修改生成的 `rupost.toml`，在末尾手动追加一行 `# my-custom-config`。再次执行 `cargo run --bin rupost -- init`  。
    *   **预期输出**：控制台输出黄色高亮 `[!] rupost.toml 已经存在于当前目录，跳过初始化。`。
    *   **结果校验**：检查文件内容，先前追加的自定义内容依旧完整保留，未被篡改。

### 2.2 验证 2：多层级变量级联覆盖优先权
1.  **准备环境**：我们直接结合 `examples/auth-flow.http` 以及 `examples/rupost.toml` 演示。
2.  **运行基本场景（使用 `rupost.toml` 中的默认配置）**：
    ```bash
    cargo run --bin rupost -- test examples/auth-flow.http --env dev
    ```
    *   **执行说明**：会使用 `examples/rupost.toml` 中 `dev` 段定义的 `base_url = "https://httpbingo.org"` 变量。
    *   **检验结果**：用例应当跑通（因为 httpbingo.org 会返回正常响应）。
3.  **运行命令行变量覆盖场景（最高优先级）**：
    ```bash
    cargo run --bin rupost -- test examples/auth-flow.http --env dev --var base_url=https://httpbin.org
    ```
    *   **执行说明**：我们将 `base_url` 在命令行中强制覆盖为 `https://httpbin.org`。
    *   **检验结果**：命令行输出中，原本指向 `httpbingo.org` 的请求全部发向了 `httpbin.org`。证明命令行变量覆盖已成功发挥最高优先级。
4.  **运行环境防命名空间污染校验**：
    ```bash
    # 注入一个与系统环境同名的不相干环境变量
    api_version="v999" cargo run --bin rupost -- test examples/auth-flow.http --env dev
    ```
    *   **结果校验**：请求依然成功向 `v1`（从 `rupost.toml` 获取）发送，而不是被外部的 `api_version=v999` 篡改，证明沙箱变量解析器对系统环境变量进行了严格命名过滤隔离。

### 2.3 验证 3：全局变量捕获 (global.) 与状态克隆 (State Cloning)
1.  **运行示例用例集**：直接跑 `examples/batch/` 目录：
    ```bash
    cargo run --bin rupost -- test examples/batch/ --env dev
    ```
    *   **观察依赖解析与执行顺序**：
        1.  由于 `02_get_profile.http` 头部声明了 `### @depends-on 01_login.http`，DAG 调度器在并行模式下会先执行 `01_login.http`。
        2.  `01_login.http` 向 `https://httpbingo.org/post` 模拟登录并携带密码 `secret_token_abc123`，并利用 `@capture global.token from body.json.password` 将其写入 `global.token` 变量。
        3.  `02_get_profile.http` 拿到 `01_login.http` 跑完之后的克隆上下文状态，成功将其渲染进 `Authorization: Bearer {{global.token}}`。
        4.  控制台最后通过两个断言，全部显示 `passed`。

### 2.4 验证 4：复杂有向依赖网 (DAG) 调度与依赖环检测
1.  **并行执行复杂拓扑用例**：
    ```bash
    cargo run --bin rupost -- test examples/batch_complex/ --env dev --mode parallel
    ```
    *   **观察拓扑控制**：
        *   系统会根据依赖图（`01_auth.http` -> `02_init_data.http` -> 并行 `03_get_category.http` / `03_publish_post_1.http` / `03_publish_post_2.http` -> `04_cleanup.http`）进行精准的多线程并行发包调度。
2.  **验证循环依赖检测**：
    *   打开 `examples/batch_complex/01_auth.http` 文件，在头部人为追加一行 `### @depends-on 04_cleanup.http`（使其与 04_cleanup 构成强连通循环依赖圈）。
    *   再次执行 `cargo run --bin rupost -- test examples/batch_complex/`。
    *   **预期输出**：终端抛出清晰的错误：`Error: Dependency loop detected`，并直接中断，避免线程死锁。
    *   **恢复文件**：撤销对 `01_auth.http` 的修改。

### 2.5 验证 5：网络诊断 waterfall 瀑布时延拆分与过期证书告警
1.  **运行高延迟诊断**：
    在 `examples/sprint1_diagnostics_demo.http` 中有一条高延迟（1秒等待）的请求。
    ```bash
    cargo run --bin rupost -- test examples/sprint1_diagnostics_demo.http --env dev --debug
    ```
    *   **观察输出**：输出的时序条（Latency Breakdown）会非常直观地展现出高延迟来源于 `Server Wait (TTFB)`（服务器内部时延占大头），而不是网络 TCP 握手。
2.  **执行 HTTPS 证书生命周期校验**：
    ```bash
    cargo run --bin rupost -- diagnose https://expired.badssl.com
    ```
    *   **观察输出**：X.509 证书段中，对于这块已过期的证书，其 Validity 会以醒目的红色输出 `[EXPIRED]`。

### 2.6 验证 6：Mock 服务端模糊路由与 Headers 变体匹配
我们将使用 `benches/load_test/mock_perf_config.json` 规则文件来验证 Mock 功能：
1.  **启动 Mock 服务**：
    ```bash
    cargo run --bin rupost -- mock benches/load_test/mock_perf_config.json --port 9000
    ```
    *   此时 Mock 服务会拉起在本地端口 9000。
2.  **验证 Trie 路径参数模糊匹配 & 变体匹配成功情况**：
    打开另一个终端发送：
    ```bash
    curl -i -X GET http://127.0.0.1:9000/api/users/12345 -H "X-Role: Admin"
    ```
    *   **期望返回**：`HTTP/1.1 200 OK`，且 Body 为 `{"status":"success","role":"Admin","user_id":"12345"}`（验证了模糊匹配的路径参数成功捕获并渲染回 Body，且 Header 匹配变体分支正确生效）。
3.  **验证 Mock 兜底分支拦截**：
    ```bash
    curl -i -X GET http://127.0.0.1:9000/api/users/12345 -H "X-Role: Guest"
    ```
    *   **期望返回**：`HTTP/1.1 403 Forbidden`，且 Body 为 `{"error":"Access Denied"}`（由于 role 不是 Admin，自动滑入 condition 为 null 的兜底分支）。

---

## 3. 一键式全特性集成验证脚本 (verify_features.sh)

在测试和开发过程中，如果想快速判定所有的特性是否有 regression（功能退化），我们提供了一键化的端到端测试与特性演示脚本：

### 3.1 运行方式
在项目根目录下，直接执行：
```bash
chmod +x tests/verify_features.sh
./tests/verify_features.sh
```

### 3.2 脚本核心验证逻辑
为了摆脱公共网络不稳定的影响，该集成脚本将整体测试转化为**纯本地自闭环测试**：
1.  自动在当前工作区编译出最新的 release 二进制。
2.  在 `mktemp -d` 隔离工作区中创建临时测试沙箱——绝不移动/覆盖仓库根的 `rupost.toml`/`.env`，历史记录同样收进工作区（`RUPOST_HISTORY_DIR`）。
3.  验证 `rupost init` 命令能否成功在空目录下初始化 `rupost.toml`，并测试防覆盖校验是否生效。
4.  在后台拉起 Mock 服务器。
5.  触发 Mock 路径参数捕获（`:id` 动态渲染）与 `X-Role: Admin` / `Guest` 变体多路条件匹配。
6.  对本地处于活动状态的 Mock 服务端口执行 `rupost diagnose` 网络诊断，检验条形时延图与状态分析。
7.  在该 Mock 服务上跑包含 Fork-Join DAG 拓扑关系的并行测试。以 **mock 变体语义判别 + 严格 exit 0** 断言业务事实：`Authorization` 精确变体证明登录捕获的 token 真实传递（State Cloning），`X-Test-Key=env-val` 精确变体证明 `.env` 真实覆盖 toml 配置——任一失守即非零退出，不以文件名出现为成功条件。
8.  `trap cleanup EXIT INT TERM` 幂等清理全部**本脚本启动**的进程与临时工作区——绝不 `killall`，开发者独立进程零干扰；先起独立 `rupost mock` 再跑脚本，脚本结束后该进程必须仍存活。
9.  全部 Mock 端口经 `RUPOST_TEST_PORT_BASE` 可平移（默认 9000–9005），固定 `sleep` 换为有限重试健康探针；端口被占用立即给出可读错误而非误杀其他进程。

---

## 4. 测试套件执行与代码覆盖率

### 4.1 运行所有 Rust 原生测试
在项目根目录下，直接运行 cargo 测试指令：
```bash
cargo test
```

`tests/incident_bundle_test.rs`（复现包契约+digests）、`tests/source_location_test.rs`（行号定位）、`tests/ast_command_test.rs`（AST 桥）、`tests/plugin_validate_test.rs`（作者校验）、`tests/cli_plugin_route_test.rs`（RUPOST_BIN 注入）、SSE/WS 行号断言（sse_assertion/websocket_integration 各一）均已入库；CI 侧：Web 前端无依赖原生单测已进独立 `web-assets` job（`node --test tests/web/app_test.mjs`，
与本地 `make test-web` 同口径）。差异化 Phase 0/1 批次新增守护：
* `tests/incident_bundle_test.rs` — 故障复现包端到端契约（失败 exit 1 + 三文件 + manifest 契约精确 + 原始凭据零泄漏 + `replay` 直接消费 + 全绿不建目录 + 目标已存在/symlink 拒绝保护）；
* `tests/web_session_reuse_test.rs::test_web_session_does_not_share_captured_variables_between_clients` — Web 双浏览器会话隔离守护（A1：跨客户端不得互读 Cookie 或 `@capture` 变量；已通过临时破坏隔离验证会确定性变红）。

### 4.2 运行代码覆盖率统计
**一键入口（推荐）**：仓库根目录提供 `make coverage`（DX 批交付，本地可选、不进 CI 门禁）：
```bash
make coverage
```
使用 `cargo-llvm-cov --workspace --summary-only` 输出汇总；未安装时打印一行安装指引。测试用 `--workspace` 与 CI 门禁同口径（含子 crate）。

**手动方式（tarpaulin，HTML 逐行视图）**：
1.  **安装工具**（若未安装）：
    ```bash
    cargo install cargo-tarpaulin
    ```
2.  **生成覆盖率报告**：
    ```bash
    cargo tarpaulin --out Html --output-dir target/tarpaulin/
    ```
    运行结束后，您可以在浏览器中打开 `target/tarpaulin/tarpaulin-report.html` 查看每一行代码的覆盖状态。

---

## 5. 新增功能完整守护套件（2026-11 整理）

围绕 CLI-parity 与 TUI 流式调试两大批次，建立了按 PRD 需求编号（R 系列）对应的功能守护矩阵。全部可离线运行（wiremock / TcpListener / ratatui TestBackend），无需真实外网。

### 5.1 守护矩阵速查

| 套件 / 所在文件 | 守护功能 | 快速运行 |
| :--- | :--- | :--- |
| `tests/tui_features_full_test.rs` | **九项功能全量守护**：多请求执行簿记与 `is_loading` 生命周期（R2/R3）、cookie 会话延续（R5，服务端实收断言）、timing 四段渲染（R6）、失败自动切 Diagnose tab（R7）、X 导出脱敏决策表（R8）、快照脱敏落盘（R9）、History 生成可解析（R9）、mock 开关真实 HTTP 往返（R11） | `cargo test --test tui_features_full_test` |
| `tests/tui_ws_session_test.rs` | **WS 交互会话**：echo 回环 + 授权握手捕获（`accept_hdr_async`）+ 非 WS/死端口拒绝 + 关闭通知 | `cargo test --test tui_ws_session_test` |
| `tests/tui_smoke_test.rs`（52 项） | **TestBackend 渲染冒烟**：布局三态、主题轴（Dark/Light 取色差异）、消息搜索高亮、Auth/Search/WsSend/Selector 模态矩阵、Results/Diagnose tab、快照跳转可见性、新键臂 | `cargo test --test tui_smoke_test` |
| `src/tui/handlers/editor.rs`（selector_wiring/wiring/quick_auth/ws_orchestration 测试模块） | **接线层**：触发链注入实收、选择器 `#2` 归一化后仅选中端点收流量、越界零流量浮错、W/S/D/A 全键编排、互斥 | `cargo test --lib tui::handlers` |
| `src/tui/state/search.rs`、`stream.rs` | **流式与搜索纯函数**：增量折行位点、CJK、1MB 截断复位、WS 溢出重建、环绕跳转决策表、吸底暂停 | `cargo test --lib tui::state` |
| `tests/capture_integration_test.rs` | **变量捕获**：逐条降级（坏的进 warnings 不污染 success）、变量名含 `from`、regex 源内含 `from`、`global.` 跨文件可见 | `cargo test --test capture_integration_test` |
| `tests/cookie_persistence_test.rs` | **cookie 持久化契约**：串行逐文件落盘 / 并行不落盘 / 触发运行中的外部观测 | `cargo test --test cookie_persistence_test` |
| `tests/report_schema_test.rs` | **JSON 报告 schema**：批量报告与 diagnose 报告精确键集合、失败形态、Duration 序列化 | `cargo test --test report_schema_test` |
| `tests/sse_stream_resilience_test.rs` | **SSE 韧性**：EOF/RST 断流尾帧 flush、1MB 单事件截断复位、无换行残片截断 | `cargo test --test sse_stream_resilience_test` |
| `tests/single_request_test.rs` | **单请求重跑**：`t case.http#3` / `--only <n\|@name>` 片段与选择器契约 | `cargo test --test single_request_test` |

### 5.2 分层验证模型（如何跑）

1. **纯函数 / 状态机层（L0–L1）**：`cargo test --lib`——秒级，提交前必跑；
2. **接线 / 集成层（L2）**：`cargo test --workspace`——CI 门禁同口径（含子 crate 与全部 tests/），本地约 1–2 分钟；
3. **渲染冒烟（TestBackend）**：`cargo test --test tui_smoke_test`——多尺寸 × 主题 × 模态矩阵，改 UI 后必跑；
4. **真机手工冒烟（L5）**：`doc/tui/usage_and_testing.md` 的三组清单（流式调试 / CLI 能力接入 / WS 交互），覆盖 IME、真实光标、外网、WS 重连等 TestBackend 不可达项。

### 5.3 红验证纪律（本批次方法论沉淀）

功能守护测试要求"改坏即红"——写完后临时破坏被测实现跑一次，确认测试变红再还原：

- 例：验证 `inject_quick_auth` 接线时，临时删除触发链中的注入调用 → wiremock 实收断言必须变红；
- 例：验证完成态日志跳转锚点时，临时还原旧的窗口局部长度钳制 → 深行号跳转可见性断言必须变红；
- 还原后必须 `diff` 基线确认实现零残留（历史教训：回写静默失败导致两测试无感知变红）。

### 5.4 环境哨兵（敌意网络下的 flake 防护）

依赖"连接被拒绝"这一网络行为的契约测试（如诊断退出码）需带环境哨兵：测试前裸 `TcpStream::connect` 探测——若透明代理/VPN 截胡使目标"可连"，打印跳过说明而非硬断言。样例见 `test_run_diagnose_cli_error_exit_one`。
