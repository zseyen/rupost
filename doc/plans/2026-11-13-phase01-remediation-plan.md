# RuPost Phase 0/1 缺口修复 + Phase 2/3 MVP 执行计划（接手文档）

**日期：** 2026-11-13
**状态：** Ready for implementation
**前置基线：** 差异化 Phase 0+1 已交付（提交链：`docs(plans)` → `fix(web)` → `test` → `feat(history)` → `feat(cli)` → `docs`；`@` 工作副本为空，lib 683 测试全绿，7/7 门禁通过）
**前置文档：** [竞品与差异化开发报告](2026-11-13-competitive-differentiation-report.md)、[差异化路线执行计划](2026-11-13-differentiation-execution-plan.md)

---

## 1. 目标、范围与完成定义

本计划分三批交付：**批一**修复交付后查漏补缺确认的四个缺口；**批二**落地竞争报告规划的 **Phase 2 MVP（文档契约门禁）** 与 **Phase 3 MVP（互操作桥接）**；**批三**收尾卫生与生态项（缺口 5/6、Web 复现包保留上限、前端开关、目录文档覆盖）。三批都是严格限界的 MVP 切片，范围边界见各 WP 与本节非目标。

### 批一：四缺口

| # | 缺口 | 现状 | 修复后 |
| --- | --- | --- | --- |
| 1 | 会话 Cookie 30 分钟硬过期 | 服务端 TTL 滑动，但 `Set-Cookie` 只在首次创建时下发，`Max-Age=1800` 从首次起算不续期——连续使用超 30 分钟后浏览器丢号牌，捕获变量丢失 | 每次执行响应重发 Set-Cookie（滑动续期），活跃会话只要间隔 <30 分钟永不过期 |
| 2 | 复现包只有 CLI 入口 | `--incident-out` 仅挂在 `test` 命令；Web 工作台跑失败拿不到工件 | Web 套件端点支持 `incident` 开关，失败时在服务端管辖目录产出三文件复现包 |
| 3 | `--env` → manifest.environment 无 CLI e2e | 领域层单测已锁，但 CLI→领域这一跳没有端到端覆盖 | 子进程 e2e：`--env staging` → manifest 与 snapshot 的 `environment` 均为 `staging` |
| 4 | exit 2 语义重叠 | clap 参数错误 / AST 解析错误 / 工件创建失败共享退出码 2，脚本无法区分 | 文档登记三来源的 stderr 鉴别前缀与 CI 判读规则（文档型 WP，无代码改动） |

### 批二：Phase 2/3 MVP

| 批次 | 内容 | 性质 |
| --- | --- | --- |
| Phase 2 MVP | ① 请求来源行号与稳定定位（`<path>#L<行号>`，终端 `path:line` + JSON `source_line`）② 契约漂移基线门禁（`--contract-baseline`：首跑建基线、复跑报漂移并更新） | 新能力 |
| AST 桥（使能器） | `rupost ast <FILE>` 输出 AstDocument JSON v1.0 + 插件环境注入 `RUPOST_BIN`——插件命令复用核心解析器，消除第三方转换工具的解析分叉（评估见 [插件承载能力评估](2026-11-13-plugin-readiness-assessment.md) §6） | 新能力（插件生态使能） |
| Phase 3 MVP | ① OpenAPI(JSON) 导入为可审查 Markdown（复用既有 `ImportFormat::OpenApi` 格式与自动检测——现状只有检测无实现）② `export k6` 场景草稿导出（与 curl/httpie/wget 同 UX 的新 target） | 新能力 |

### 批三：卫生与生态收尾

| 批次 | 内容 | 性质 |
| --- | --- | --- |
| 缺口 5 收尾 | `examples/run_all.sh` 端口平移（`RUPOST_EXAMPLE_PORT_BASE`）+ 健康探针替代 `sleep 2`（incident 段已由 d8913f0f 先行完成，本批补齐 llm/sse/plugin 段） | 可信度对齐 |
| 缺口 6 | manifest 完整性摘要 `artifact_digests`（sha256，只防损坏/无意篡改；防恶意篡改需密钥签名，登记不做） | 领域层追加 |
| 安全残余收口 | Web 复现包保留上限（最旧裁剪，32 个；仅 Web 通道，CLI 不适用） | 安全分析残余风险 |
| 前端开关转正 | 工作台套件面板 incident 复选框（原 WP-R2 可选加分项；可裁剪） | 辅助执行体验 |
| 目录文档 | `directory_structure.md` 补 `src/web` 等缺失模块条目 | 文档债 |

### 总体验收

| 编号 | 验收事实 |
| --- | --- |
| A-R1 | 携带有效会话 Cookie 的后续执行响应重发 `Set-Cookie`，会话 ID 不变；执行前失败（只读/解析错误）仍不触碰 Cookie |
| A-R2 | Web 套件失败 + `incident:true` → `<working_dir>/.rupost/incidents/` 下产出三文件；全绿或无开关不产目录；客户端无法指定输出路径；`masking` 恒为 `secrets` |
| A-R3 | CLI e2e 断言 `manifest.environment` 与 `snapshot.environment` 均等于 `--env` 值 |
| A-R4 | debug_guide / 示例 README 中三来源 exit-2 鉴别表与 CI 判读规则可查 |
| A-P2a | 失败请求在终端报告输出 `doc.md:10` 形态定位、JSON 报告新增 `source_line` 字段；`.md` 与 `.http` 两条解析路径都有表驱动锁定 |
| A-P2b | `--contract-baseline` 首跑建基线（stderr 提示）、复跑报漂移（`⚠ 契约漂移: N 条…`）并更新基线；基线文件字段精确集 = `schema_version` + `contracts[]{id, assertion, passed}`，无任何响应 actual 值；漂移不改变退出码 |
| A-P3a | OpenAPI JSON spec → `rupost import openapi spec.json -o out.md` 产出可审查 Markdown（路径参数/凭据模板化、status 断言、防覆盖）；web `/api/v1/import` 复用同一实现 |
| A-P3b | `.http` → `rupost export k6 <file> -o script.js` 产出可运行脚本骨架（`check()` 断言、`__ENV` 模板变量、负载模型为 stub 注释）；`-o` 防覆盖（`--force` 覆写） |
| A-R6 | `RUPOST_EXAMPLE_PORT_BASE=18080 ./examples/run_all.sh` 与默认端口两场景全 PASS；脚本内无写死的 8080/8090/8095 |
| A-R7 | 失败运行产出的 manifest 含 `artifact_digests`，恰覆盖 report/snapshot 两文件且 sha256 与文件字节独立计算一致；schema 契约测试键集同步（10→11） |
| A-R8 | 32 个服务端命名假目录 + 一次真实失败 incident 运行 → 最旧被裁剪、非命名目录与 symlink 不被触碰；CLI 通道不受影响 |
| A-R9 | `directory_structure.md` 出现 `src/web`（及缺失模块）条目，含薄表现层定位与扩展缝指针 |
| A-R5 | fmt、clippy、`cargo test --workspace`、`make test-web`、`verify_features.sh`、`examples/run_all.sh`、`git diff --check` 全绿 |

### 非目标（明确不做，防止范围蔓延）

- **TUI 不加复现包 / 契约漂移入口**：TUI 面向单请求调试（已有 Ctrl+S 快照覆盖）。
- **Phase 2 边界**：不做 heading-slug 锚点（行号即定位）；摘要**不存任何 actual 值**（动态字段问题从根上规避，无需掩码）；漂移只打 stderr 摘要，不改变退出码（三值契约 0/1/2 不动）；快照 Schema v1.0 不动。
- **Phase 3 边界**：OpenAPI 仅支持 **JSON spec**（YAML 需评估 serde_yaml 新依赖，登记不做）；请求体只取 `example`/`examples`/`default`，不做 schema 深度合成；k6 只产场景草稿——**不生成负载模型（vus/stages/thresholds）、不实现虚拟用户/分布式调度/指标库**（战略"明确不做"延续）。
- **P2/P3 示例不接入 run_all.sh**（维持登记）；**run_all.sh 端口平移（缺口 5）与 manifest 完整性摘要（缺口 6）已纳入批三**（WP-R5/WP-R6）——注意摘要（digest）只防损坏，**密钥签名级防篡改仍不做**。
- 不扩第四个退出码值；不动 `session.rs` 存储结构、CLI `--incident-out` 语义、`verify_features.sh`。
- 不做云服务/账号/团队空间。

---

## 2. 固定决策（含两处契约修订）

### 2.1 Cookie 滑动续期（修订原执行计划 WP-1 步骤 6）

原文"仅首次创建下发 Set-Cookie"修订为：

> `/api/v1/run/block` 与 `/api/v1/run/suite` 的**每次执行响应**（成功与业务失败，含 suite 的 500 执行错误）都重发 `Set-Cookie`；Cookie 值与会话 ID 不变，`Max-Age=1800` 随每次响应重新起算。执行前失败（只读拒绝、解析错误等，未到达会话层）仍不触碰 Cookie。

理由：服务端 TTL 本就按 `last_access` 滑动，续期使浏览器侧与服务端语义对齐。Cookie 属性串、TTL 常量、会话 ID 生成方式一律不变。

### 2.2 Web 复现包路径与密钥通道（新增决策）

- **客户端零路径输入**：请求体只接受布尔开关 `incident`，绝不接受路径——浏览器可控的文件系统路径是注入/滥用面。输出位置固定由服务端决定：`<working_dir>/.rupost/incidents/<UTC时间戳>-<uuid前8位>/`（`.rupost` 与 vars.json 同惯例；时间戳+uuid 保证不冲突，配合"目标必须不存在"原语双保险）。
- **浏览器不是可信 secret 通道**：Web 侧 `secret_values` 恒为空（不收任何值清单），仅键名掩码生效；`masking=secrets` 仍强制，契约不降级。
- **仅挂 suite 端点**：复现包语义是批量工件（含上游登录上下文），单块调试不产包。
- **失败语义镜像 CLI**：工件创建失败 → HTTP 500 + `ApiResponse::err`（fail-closed，对应 CLI 的 exit 2），不静默吞掉。
- **全绿不产目录**：与 CLI 完全一致。

### 2.3 Schema 只追加

新字段一律 `#[serde(default)]`，不改既有字段语义：`RunSuiteRequest.incident: Option<bool>`、`RunSuiteResponse.incident_dir: Option<String>`。旧客户端不带新字段照常工作。

### 2.4 稳定定位与契约摘要契约（Phase 2）

- **`source_line` 定义**：请求 HTTP 方法行的 1-based 行号（块内首个非空且非 `#` 开头的行）。`.http`（`###` 分隔，parser/http_file.rs）与 `.md`（` ``` ` 围栏，parser/markdown_file.rs）**两条解析路径都必须覆盖**。
- **稳定 ID 字符串**：`<normalize_posix_path>#L<行号>`；`source_line` 缺失时退化为 `<path>#<request_number>`。
- **Schema 追加**（只增不改）：`RequestMetadata.source_line: Option<usize>` → `TestResult.source_line: Option<usize>`（三个构造器内置 None，http_runner.rs:202/:255 两处构造后回填——两处已是 `let mut test_result` 绑定，零调用点迁移）→ `JsonTestReport.source_line`（serde default）。**快照 Schema v1.0 不动**。
- **契约摘要（ContractSummary）字段精确集**：`schema_version` + `contracts[]{id, assertion, passed}`——**不存 actual 值**；漂移 diff 以 `id::assertion` 为键，断言原文改变视同新增。
- **漂移语义**：基线 `passed=true` → 本次 `false` 为 newly_broken；反向为 fixed；基线无此键为 new。只打 stderr 摘要，退出码不变。
- 基线文件写入走 `security::fs::write_private_file_atomic`（0600，可覆写更新）；读取缺失文件 = 首跑（无基线）。

### 2.5 互操作契约（Phase 3）

- **OpenAPI**：实现挂在**既有** `ImportFormat::OpenApi` 之上——枚举与自动检测已存在（importer/mod.rs:33、:82-87），**现状只有检测没有实现**（src/importer/ 下仅 curl.rs/httpie.rs/raw.rs）。仅支持 JSON spec；生成物为**单一可审查 Markdown**（非私有集合）：
  - 路径参数 `{x}` → `{{x}}`；bearer securityScheme → `Authorization: Bearer {{token}}`；server base → `{{base_url}}`；
  - 请求体取 `example`/`examples`/`default`，否则 `{}` + ImportResponse warning；
  - 响应取最低 2xx 码 → `# @assert status == <code>`，无 2xx → 无断言 + warning；
  - `operationId` → `# @name`；防覆盖复用 import 既有链路（curl 同款）。
  - 实现后 web `/api/v1/import` 因自动检测已存在而免费获得该能力。
- **k6**：以 export 子系统**新 target `"k6"`** 交付（`rupost export k6 <file> -o out.js`，与 curl/httpie/wget 完全同 UX，`--force`/`--redact`/`--name` 等旗标自动继承）。产物为场景草稿：
  - `options` 恒为 `{ vus: 1, iterations: 1 }` stub + 注释声明"负载模型由使用者扩展"；
  - `{{base_url}}` → `` `${BASE_URL}` ``（`BASE_URL = __ENV.BASE_URL || ''`）；其余 `{{var}}` → `__ENV.VAR`；
  - `status == N` 断言 → `check()`；其余 `@assert` → JS 注释 `// TODO: @assert <raw>`；
  - `@skip` 与 teardown 请求排除；`@capture` → 注释提示"k6 中变量传递为脚本职责，需手动编排"。

### 2.6 批三固定决策（卫生与生态收尾）

- **完整性摘要 ≠ 签名**：`artifact_digests` 是 sha256 摘要，只防传输损坏/无意篡改；防恶意篡改需密钥签名（登记不做，命名与文档必须用 digest 不用 signature）。digests 覆盖 `report.json` + `snapshot.json`；manifest 自身无法自含摘要（消费方语义：两工件摘要匹配 + manifest 存在即完整）。
- **复现包保留上限仅作用于 Web 通道**：CLI `--incident-out` 是用户显式路径，不做保留策略（不惊讶原则）；Web 侧创建前按"服务端命名模式"裁剪最旧（32 个），**只删匹配命名的目录**，用户手工放入的内容与 symlink 一律跳过。
- **前端开关只多 1 bit**：复选框仅改变 payload 的 `incident` 字段与提示渲染（渲染必须走既有 escapeHtml），不新增任何端点或路径输入。
- **示例端口一个旋钮**：`RUPOST_EXAMPLE_PORT_BASE`（默认 8080），LLM/SSE=+0、plugin=+10；默认值下行为与历史完全一致。与 `verify_features.sh` 的 `RUPOST_TEST_PORT_BASE` 是两个独立脚本各自旋钮，不合并。

### 2.7 AST 桥契约（WP-AST，插件生态使能）

- **形态裁决**：`rupost ast <FILE>` 只读命令 → stdout JSON（AstDocument v1.0）；插件命令经内核注入的 `RUPOST_BIN`（`std::env::current_exe()`，plugin_exec.rs:195-211 白名单重建处追加，约 2 行）自调内核获取 AST。**不做** stdin AST 注入协议（命令模式今天是裸 argv 直通 + TTY 继承，结构化 stdin 与 `requires_tty` 冲突且需版本化协议）；**不做** Rust 库 SDK 承诺。评估全文：[插件承载能力评估](2026-11-13-plugin-readiness-assessment.md) §6。
- **安全面零扩大**：命令插件本就能读工作区文档原文（`RUPOST_WORKSPACE` 已知、文件在盘上）；AST 桥只共享解析代码，不新增数据访问。
- **契约纪律**：不做解析器类型直接 derive（`ParsedFile/ParsedRequest/RequestMetadata` 均未实现 Serialize，直接 derive 会把内部命名泄漏为公共契约），建**专用投影** `AstDocument`，`schema_version "1.0"` 只增不改（与 snapshot/incident/contract 同纪律）。投影层隔离内部类型重构与外部契约。
- **依赖**：AST 暴露 `source_line`，排在 WP-P2a 之后；P3a/P3b 默认核心内置结论不变——AST 桥定位为插件生态使能器（C-R4 后可做一方 showcase 插件实验，验证"他人用插件实现功能"闭环）。

---

## 3. 工作包

**执行顺序：批一 WP-R3 → WP-R1 → WP-R2 → 批二 WP-P2a → WP-AST → WP-P2b → WP-P3a → WP-P3b → 批三 WP-R5 → WP-R6 → WP-R7 → WP-R8（可裁剪）→ WP-R9 → WP-R4**（缺口修复先行；Phase 2 的漂移依赖定位字段先落，故 P2a 在 P2b 前，AST 桥暴露 source_line 亦排 P2a 之后；Phase 3 两个 WP 相互独立；批三依赖 R2 已落（R7/R8），其余相互独立；WP-R4 文档收尾最后覆盖全三批。每个 WP 独立提交）。
本计划文档已入库（提交 `docs(plans): add phase 0/1 remediation plan (gaps 1-4)`，59159329）；状态行随 WP-R4 收尾更新。

> **审查记录（2026-11-13，执行前核对）：** 剩余 11 个 WP 的前提已对当前 HEAD 实测核对——P2a（http_runner.rs:202/:255 `mut` 绑定精确命中、解析器未 Serialize、json.rs 无 source_line）、AST（builtin 名单 plugin_route.rs:376-384、env 注入 plugin_exec.rs:207-211）、P2b（incident 块锚点 suite_presenter.rs:129-137 成立；TestSuiteOptions 构造点 html_report×3 + dispatch×1）、R5（run_all.sh 8080×7 / sleep×3，incident 段已有 `RUPOST_INCIDENT_MOCK_PORT`）、R6（manifest 10 字段、无 digests）、R7（run.rs:338/344 前提成立）、R8（app.js escapeHtml/runSuite 存在）、R9（src/web 计数 0）——全部成立。**两处骨架已按现场事实修正并写入对应 WP**：P3a（ImportRequest 无 output 字段、ImportResponse 产出 markdown_blocks 向量）、P3b（逐请求 CommandExporter 架构 → file_preamble 默认方法；管线变量已插值 → 取消 `__ENV.VAR` 映射与 check() 翻译，凭据纪律 `--redact`）。P2a 附带必做项确认：`report_schema_test.rs` 采用精确键集合断言，`source_line` 必须显式登记。R4 范围确认：批一（R1/R2/R3）尚未入 CHANGELOG，随 R4 登记。

### WP-R3（缺口 3）：`--env` 贯通 CLI e2e（热身，纯测试）

**目的：** 锁定 `--env` 从 CLI 到 manifest/snapshot 的端到端贯通。现状实现已贯通，本 WP 是纯覆盖补齐——**不写任何生产代码**。

**文件：** 只改 `tests/incident_bundle_test.rs`（追加一个测试）。

**先写测试（完整代码，直接追加到文件末尾）：**

```rust
/// WP-R3（缺口 3）：--env 必须贯通到 manifest.environment 与 snapshot.environment。
/// 反假守护：把 src/cli/suite_presenter.rs 中传给 create_incident_bundle 的
/// options.env.as_deref() 临时改为 None，本测试必须稳定变红；验证后恢复。
#[tokio::test]
async fn test_incident_manifest_carries_env_from_cli_flag() {
    let server = MockServer::start().await;
    mount_failing_charge(&server).await;

    let workdir = tempdir().unwrap();
    let history = tempdir().unwrap();
    let artifact_root = tempdir().unwrap();
    write_failing_fixture(workdir.path(), &server.uri());
    // --env 需要可解析的环境名：最小 rupost.toml 声明 staging
    std::fs::write(
        workdir.path().join("rupost.toml"),
        format!("[environments.staging]\nbase_url = \"{}\"\n", server.uri()),
    )
    .unwrap();

    let incident_path = artifact_root.path().join("with-env");
    let out = run_rupost(
        workdir.path(),
        history.path(),
        &[
            "t".into(),
            "payments.http".into(),
            "--env".into(),
            "staging".into(),
            "--incident-out".into(),
            incident_path.to_string_lossy().into_owned(),
            "--mask-secret-value".into(),
            SECRET_TOKEN.into(),
        ],
    );
    assert_eq!(
        out.status.code(),
        Some(1),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let manifest_bytes = std::fs::read(incident_path.join("manifest.json")).unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes).unwrap();
    assert_eq!(manifest["environment"], "staging", "manifest: {manifest}");

    let snap_bytes = std::fs::read(incident_path.join("snapshot.json")).unwrap();
    let snap: serde_json::Value = serde_json::from_slice(&snap_bytes).unwrap();
    assert_eq!(snap["environment"], "staging");
}
```

**必须避免：** 若测试意外变红，说明存在真实回归——修实现，不许删测试或改断言迁就。

**反假守护（必须执行并记录）：** 临时把 `src/cli/suite_presenter.rs` 中 `create_incident_bundle(...)` 调用的 `options.env.as_deref()` 改为 `None` → 跑测试必须红 → 恢复 → 绿。

**验证与提交：**

```bash
cargo test --test incident_bundle_test
# （中间执行上述反假守护，记录红/绿结果）
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
jj split tests/incident_bundle_test.rs -m "test(cli): lock --env propagation into incident manifest"
```

### WP-R1（缺口 1）：会话 Cookie 滑动续期

**目的：** 活跃会话不再因 Cookie 固定倒计时在 30 分钟硬过期。

**文件：**

- 修改 `src/web/handlers/run.rs`（`json_session_response` 一处 + 注释）
- 修改 `tests/web_session_reuse_test.rs`（模块注释 + 追加一个测试）

**先写测试（追加到 `tests/web_session_reuse_test.rs` 末尾；`run_block_full` helper 已存在于该文件）：**

```rust
/// WP-R1（缺口 1）滑动续期守护：携带有效会话 Cookie 的后续执行响应必须重发
/// Set-Cookie（同 ID；Max-Age=1800 随响应重新起算）——活跃会话不得因 Cookie
/// 固定倒计时在 30 分钟硬过期。
/// 反假守护：把 run.rs json_session_response 恢复 `.filter(|h| h.created)`
/// （仅首次下发）时，本测试必须稳定变红。
#[tokio::test]
async fn test_web_session_cookie_renewed_on_active_use() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/login"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"token":"tok-r1"}"#))
        .mount(&server)
        .await;

    let app = create_web_router(WebServerConfig::default());
    let login_block = format!(
        "### login\nPOST {}/login\nContent-Type: application/json\n\n{{}}\n\n@capture auth_token from body.token\n",
        server.uri()
    );

    let (_, first) = run_block_full(&app, "login.http", &login_block, None, None).await;
    let cookie = first.expect("首次执行必须下发会话 Cookie");

    let (_, renewed) = run_block_full(&app, "login.http", &login_block, None, Some(&cookie)).await;
    let renewed = renewed.expect("活跃会话的后续执行必须重发 Set-Cookie 滑动续期");
    assert_eq!(renewed, cookie, "续期必须保持同一会话 ID（不得轮换）");
}
```

同时把该文件模块注释中"首次执行下发 Set-Cookie"的表述更新为"下发并随每次执行滑动续期"。

**实施步骤（Green）：**

1. `src/web/handlers/run.rs` 的 `json_session_response`，把：

```rust
    if let Some(v) = handle
        .filter(|h| h.created)
        .map(|h| session_set_cookie_value(&h.id))
        .and_then(|c| HeaderValue::from_str(&c).ok())
    {
```

改为（删除 `.filter`，并更新函数 doc 注释为"每次执行响应滑动续期"）：

```rust
    // WP-R1：滑动续期——只要到达会话层（执行前失败 handle 为 None，自然不附），
    // 每次执行响应都重发 Set-Cookie；浏览器 Max-Age 重新起算，与服务端滑动 TTL 对齐
    if let Some(v) = handle
        .map(|h| session_set_cookie_value(&h.id))
        .and_then(|c| HeaderValue::from_str(&c).ok())
    {
```

**必须避免：** 不改 `SESSION_TTL`/Cookie 属性串；不轮换会话 ID；不给只读/解析错误路径加 Cookie（它们 `handle` 为 `None`，改动后自动满足）；不动 `session.rs`。

**反假守护：** 恢复 `.filter(|h| h.created)` → 新测试必须红 → 移除 → 绿。

**验证与提交：**

```bash
cargo test --test web_session_reuse_test
cargo test --test web_api_integration_test
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
jj split src/web/handlers/run.rs tests/web_session_reuse_test.rs -m "fix(web): renew session cookie for active sessions (sliding max-age)"
```

### WP-R2（缺口 2）：Web 工作台复现包

**目的：** Web 套件失败时可产出复现包工件，位置由服务端管辖（决策 §2.2）。

**文件：**

- 修改 `src/web/types.rs`（两个新字段）
- 修改 `src/web/server.rs`（`AppState` 加一个字段）
- 修改 `src/web/handlers/run.rs`（`run_suite_handler` 插入 bundle 步骤）
- 修改 `tests/web_api_integration_test.rs`（新增测试 + 迁移既有字面量）

**先写测试（Red 阶段即可编译：请求体用裸 `json!`，serde 对未知键 `incident` 静默忽略 → `incident_dir` 缺失 → 断言失败变红）：**

追加到 `tests/web_api_integration_test.rs`（该文件已有 `tempdir`/`Arc`/`LocalFsDocumentStore`/`WebServerConfig`/oneshot 全套导入）：

```rust
/// WP-R2（缺口 2）：Web 套件失败 + incident 开关 → 服务端管辖目录产出三文件
/// 复现包；绿跑或无开关不产；客户端无法指定路径（路径注入免疫）。
/// 反假守护：把 handler 中 `req.incident == Some(true)` 临时改为 `false`，
/// 本测试必须稳定变红。
#[tokio::test]
async fn test_web_suite_incident_bundle_emitted_on_failure() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/boom"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&server)
        .await;

    let tmp = tempdir().unwrap();
    std::fs::write(
        tmp.path().join("fail.http"),
        format!("### boom\nGET {}/boom\n\n# @assert status == 200\n", server.uri()),
    )
    .unwrap();

    let store = Arc::new(LocalFsDocumentStore::new(tmp.path()));
    let config = WebServerConfig {
        store,
        working_dir: tmp.path().to_path_buf(),
        ..Default::default()
    };
    let app = create_web_router(config);

    let payload = serde_json::json!({"path": "fail.http", "incident": true});
    let req = Request::builder()
        .uri("/api/v1/run/suite")
        .method("POST")
        .header("host", "127.0.0.1")
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let wrapped: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let data = &wrapped["data"];
    assert_eq!(data["failed"], 1);

    let incident_dir = data["incident_dir"]
        .as_str()
        .expect("失败 + incident 开关必须返回 incident_dir");
    let dir = std::path::Path::new(incident_dir);
    for f in ["manifest.json", "report.json", "snapshot.json"] {
        assert!(dir.join(f).is_file(), "missing artifact: {f}");
    }
    // 路径注入免疫：位置必须落在服务端管辖目录（contains 判定跨平台安全）
    assert!(
        incident_dir.contains(".rupost/incidents"),
        "incident_dir 必须位于服务端管辖目录: {incident_dir}"
    );
    // 强制脱敏声明 + 环境贯通（web 默认无 default_env → null）
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["masking"], "secrets");
    assert_eq!(manifest["environment"], serde_json::Value::Null);
}

/// WP-R2 成功对照：无开关（失败）或全绿（有开关）都不得产出目录。
#[tokio::test]
async fn test_web_suite_no_incident_bundle_without_flag_or_on_green() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/boom"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/ok"))
        .respond_with(ResponseTemplate::new(200).set_body_string("fine"))
        .mount(&server)
        .await;

    // 场景 A：失败但无开关 → 无目录
    let tmp_a = tempdir().unwrap();
    std::fs::write(
        tmp_a.path().join("fail.http"),
        format!("### boom\nGET {}/boom\n\n# @assert status == 200\n", server.uri()),
    )
    .unwrap();
    let config = WebServerConfig {
        store: Arc::new(LocalFsDocumentStore::new(tmp_a.path())),
        working_dir: tmp_a.path().to_path_buf(),
        ..Default::default()
    };
    let app = create_web_router(config);
    let payload = serde_json::json!({"path": "fail.http"});
    let req = Request::builder()
        .uri("/api/v1/run/suite")
        .method("POST")
        .header("host", "127.0.0.1")
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let wrapped: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(wrapped["data"]["failed"], 1);
    assert!(
        wrapped["data"]["incident_dir"].is_null(),
        "无开关不得返回 incident_dir"
    );
    assert!(!tmp_a.path().join(".rupost").exists(), "无开关不得产出目录");

    // 场景 B：开关打开但全绿 → 无目录
    let tmp_b = tempdir().unwrap();
    std::fs::write(
        tmp_b.path().join("ok.http"),
        format!("### ok\nGET {}/ok\n\n# @assert status == 200\n", server.uri()),
    )
    .unwrap();
    let config = WebServerConfig {
        store: Arc::new(LocalFsDocumentStore::new(tmp_b.path())),
        working_dir: tmp_b.path().to_path_buf(),
        ..Default::default()
    };
    let app = create_web_router(config);
    let payload = serde_json::json!({"path": "ok.http", "incident": true});
    let req = Request::builder()
        .uri("/api/v1/run/suite")
        .method("POST")
        .header("host", "127.0.0.1")
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let wrapped: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(wrapped["data"]["failed"], 0);
    assert!(wrapped["data"]["incident_dir"].is_null(), "全绿不产目录");
    assert!(!tmp_b.path().join(".rupost").exists(), "全绿不得产出目录");
}
```

**实施步骤（Green）：**

1. `src/web/types.rs`：
   - `RunSuiteRequest` 追加 `/// WP-R2：失败时产出复现包（布尔开关；输出位置由服务端决定，绝不接受客户端路径）` `#[serde(default)] pub incident: Option<bool>`；
   - `RunSuiteResponse` 追加 `#[serde(default)] pub incident_dir: Option<String>`。
2. `src/web/server.rs`：`AppState` 追加 `pub incident_root: std::path::PathBuf`；`create_web_router` 初始化处追加 `incident_root: config.working_dir.join(".rupost").join("incidents"),`。
3. `src/web/handlers/run.rs` 的 `run_suite_handler`：
   - 聚合循环 `for r in results` 改为 `for r in &results`（results 稍后移入 bundle）；
   - 在构造 `RunSuiteResponse` **之前**插入（照抄骨架）：

```rust
    // WP-R2：故障复现包（仅失败时；服务端管辖路径，客户端零路径输入；浏览器
    // 不是可信 secret 通道——secret_values 恒空，仅键名掩码，masking=secrets 仍强制）
    let mut incident_dir: Option<String> = None;
    if req.incident == Some(true) && failed > 0 {
        let target = state.incident_root.join(format!(
            "{}-{}",
            chrono::Utc::now().format("%Y%m%dT%H%M%S"),
            &uuid::Uuid::new_v4().simple().to_string()[..8],
        ));
        let batch = vec![(std::path::PathBuf::from(&req.path), results)];
        if let Err(e) = rupost::history::create_incident_bundle(
            &batch,
            &[req.path.clone()],
            state.default_env.as_deref(),
            &[],
            rupost::history::model::DEFAULT_MAX_SNAPSHOT_BODY_BYTES,
            &target,
        ) {
            // fail-closed 镜像 CLI exit-2 语义：工件失败不静默
            return json_err_session_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Incident bundle creation failed: {}", e.to_user_friendly_string()),
                Some(&handle),
            );
        }
        incident_dir = Some(rupost::utils::path_guard::normalize_posix_path(&target));
    }
```

   - `RunSuiteResponse { ... }` 构造处追加 `incident_dir,`。

4. 迁移既有字面量：`rg "RunSuiteRequest {" src/ tests/`，给每个既有点位补 `incident: None,`（web_api_integration_test.rs 的 suite 测试与 error_shapes 系列有若干处，让编译器列出）。

**必须避免：** 不接受任何路径类请求字段；不给 run/block 端点加 bundle；不在全绿时产目录；不改 `create_incident_bundle` 签名；TUI 不动。

**反假守护：** 把 handler 中条件临时改为 `if false && req.incident == Some(true) && failed > 0` → 新测试必须红 → 恢复 → 绿。

**验证与提交：**

```bash
cargo test --test web_api_integration_test
cargo test --test web_session_reuse_test     # 回归（同模块被改）
cargo test --lib web::                        # AppState/类型层
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
jj split src/web tests/web_api_integration_test.rs -m "feat(web): emit incident bundles from workbench suite runs"
```

（可选加分项，时间允许再做：`src/web/assets/app.js` 套件面板加 incident 复选框 + `tests/web/app_test.mjs` 补用例 + `make test-web`；不做不算缺口。）

### WP-P2a（Phase 2①）：请求来源行号与稳定定位

**目的：** 失败请求能精确定位到文档行——终端输出 `doc.md:10`，JSON 报告带 `source_line`；稳定 ID 为 `<path>#L<行号>`。

**文件：**

- 修改 `src/parser/types.rs`（`RequestMetadata` 加字段 + 解析层表驱动单测）
- 修改 `src/parser/http_file.rs`、`src/parser/markdown_file.rs`（两条分块路径回填行号）
- 修改 `src/runner/types.rs`（`TestResult` 加字段，构造器默认 None）
- 修改 `src/runner/protocol/http_runner.rs`（:202/:255 两处回填）
- 修改 `src/report/json.rs`（`JsonTestReport` 加字段并回填）
- 修改 `src/cli/suite_presenter.rs`（终端失败定位行）
- 新增 `tests/source_location_test.rs`（e2e）

**先写测试（Red）：**

1. e2e（运行期红——JSON `source_line` 键缺失为 null，终端无定位行）。新建 `tests/source_location_test.rs`：

```rust
//! WP-P2a 守护：失败请求可定位到文档行（终端 `path:line` + JSON `source_line`）。
mod common;

use std::process::Command;
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn run(dir: &std::path::Path, history: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rupost"))
        .env("RUPOST_HISTORY_DIR", history)
        .current_dir(dir)
        .args(args)
        .output()
        .expect("spawn rupost")
}

/// 反假守护：删除 suite_presenter 的定位行打印、或 build_json_reports 的
/// source_line 回填，本测试对应段断言必须分别稳定变红。
#[tokio::test]
async fn test_failed_request_reports_source_location() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ok"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/boom"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let dir = tempdir().unwrap();
    let history = tempdir().unwrap();
    // 行号台账：1 标题 / 2 空 / 3 围栏 / 4 GET ok / 5 空 / 6 assert / 7 围栏
    //           8 空 / 9 围栏 / 10 GET boom / 11 空 / 12 assert / 13 围栏
    let doc = format!(
        "标题\n\n```http\nGET {}/ok\n\n# @assert status == 200\n```\n\n```http\nGET {}/boom\n\n# @assert status == 200\n```\n",
        server.uri(),
        server.uri()
    );
    std::fs::write(dir.path().join("doc.md"), &doc).unwrap();

    // JSON 报告：source_line 字段（若 stdout 出现非 JSON 噪音，改用
    // --report json --report-file r.json 读文件断言，断言内容不变）
    let out = run(dir.path(), history.path(), &["t", "doc.md", "--report", "json"]);
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let report: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("json 报告应为纯 JSON 数组");
    let boom = &report[0]["results"][1];
    assert_eq!(boom["request_number"], 2);
    assert_eq!(boom["source_line"], 10, "boom 块方法行应为第 10 行: {boom}");

    // 终端报告：失败块输出 `doc.md:10` 稳定定位
    let out = run(dir.path(), history.path(), &["t", "doc.md"]);
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("doc.md:10"), "终端必须输出稳定定位: {stdout}");
}
```

2. 解析层表驱动单测（放 `src/parser/types.rs` 的 `#[cfg(test)] mod tests`；Red 形态为字段缺失的**编译失败**，把编译错误保存为红证据）：

```rust
/// WP-P2a：source_line 契约——方法行的 1-based 行号，.md 与 .http 双路径锁定
#[test]
fn test_source_line_contract_md_and_http() {
    // .md 围栏：行4 与行10 的 GET
    let md = "前言\n\n```http\nGET http://x/ok\n\n# @assert status == 200\n```\n\n```http\nGET http://x/boom\n\n# @assert status == 200\n```\n";
    let parsed = crate::parser::parse_document_by_path("doc.md", md).unwrap();
    assert_eq!(parsed.requests.len(), 2);
    assert_eq!(parsed.requests[0].metadata.source_line, Some(4));
    assert_eq!(parsed.requests[1].metadata.source_line, Some(10));

    // .http ### 分隔：行3 与行6 的 GET
    let http = "### first\n# @assert status == 200\nGET http://x/a\n\n### second\nGET http://x/b\n";
    let parsed = crate::parser::parse_document_by_path("a.http", http).unwrap();
    assert_eq!(parsed.requests.len(), 2);
    assert_eq!(parsed.requests[0].metadata.source_line, Some(3));
    assert_eq!(parsed.requests[1].metadata.source_line, Some(6));
}
```

**实施步骤（Green）：**

1. `src/parser/types.rs`：`RequestMetadata` 追加 `/// WP-P2a：请求方法行的 1-based 行号（稳定定位 <path>#L<line>）` `pub source_line: Option<usize>`；`rg "RequestMetadata {" src/parser/` 让编译器/搜索列出构造点，默认 `None`。
2. `src/parser/http_file.rs` 与 `src/parser/markdown_file.rs`：`rg "ParsedRequest {" src/parser/` 找分块构造点；在扫描循环中记录当前行索引，按 §2.4 定义（方法行 = 块内首个非空且非 `#` 开头的行）回填。**两条路径都要覆盖**。
3. `src/runner/types.rs`：`TestResult` 追加 `pub source_line: Option<usize>`，三个构造器（success/error/skipped）置 `None`——调用点走构造器，**零迁移**。
4. `src/runner/protocol/http_runner.rs`：:202（`TestResult::success`）与 :255（`TestResult::error`）构造后回填 `test_result.source_line = parsed.metadata.source_line;`（变量名以现场为准）。sse/ws 回填登记后补，本批不做。
5. `src/report/json.rs`：`JsonTestReport` 追加 `#[serde(default)] pub source_line: Option<usize>`，`build_json_reports` 中回填。
6. `src/cli/suite_presenter.rs` 终端分支 `reporter.print_result(r)` 之后追加：

```rust
                // WP-P2a：失败请求的稳定定位（json 模式走字段，终端给行号）
                if !r.success {
                    if let Some(line) = r.source_line {
                        println!("    {}:{line}", path_str);
                    }
                }
```

**必须避免：** 不动快照 Schema（快照不加 source_line）；不改解析器/报告的既有公共签名（新字段 + 回填即可）；不引入 slug/锚点逻辑。

**反假守护：** 分别临时删除 suite_presenter 定位打印与 json.rs 回填 → e2e 两段断言分别红 → 恢复 → 绿。

**验证与提交：**

```bash
cargo test --lib parser::types
cargo test --test source_location_test
cargo test --workspace            # JsonTestReport 加字段可能影响 report_schema_test——键集追加属契约允许
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
jj split src/parser src/runner src/report src/cli tests/source_location_test.rs -m "feat(parser,report): stable source location for requests (path#L<line>)"
```

（**必做项（审查确认）**：`tests/report_schema_test.rs` 采用**精确键集合断言**且明确要求新增字段显式更新契约——`source_line` 落地时必须同步登记该测试，否则其必红；属 schema 只追加的契约内变更。）

### WP-AST（AST 桥，使能器）：`rupost ast` JSON 契约与 `RUPOST_BIN` 注入

**目的：** 向插件命令开放核心解析结果——第三方转换工具（k6/postman/har/docs 生成器等）经 `rupost "$RUPOST_BIN" ast file.http` 获取解析 AST，**解析语义与核心零分叉**。决策依据与形态对比见 [插件承载能力评估](2026-11-13-plugin-readiness-assessment.md) §6 与本计划 §2.7。依赖 WP-P2a（`source_line`）。

**文件：**

- 新增 `src/parser/ast_json.rs`（`AstDocument`/`AstRequest` 投影 + 序列化 + 契约单测）
- 修改 `src/parser/mod.rs`（`pub mod ast_json;` + re-export）
- 修改 `src/cli/commands/mod.rs`（`Commands::Ast`）、`src/cli/dispatch.rs`（分发：读文件 → 解析 → stdout JSON）
- 修改 `src/cli/plugin_exec.rs`（env 注入 `RUPOST_BIN`）
- 修改 `tests/cli_plugin_route_test.rs`（扩展既有 env 白名单测试断言 `RUPOST_BIN` 注入）
- 修改 `doc/architecture/features/plugin_and_extensibility.md`（§3 作者指南补"用 AST 桥消除解析分叉"一节）
- 新增 `tests/ast_command_test.rs`（e2e）

**契约（AstDocument v1.0，只增不改）：**

```json
{
  "schema_version": "1.0",
  "source_path": "suite.http",
  "dependencies": [],
  "requests": [
    {
      "number": 1,
      "method": "GET",
      "url": "{{base_url}}/ok",
      "headers": [["Authorization", "Bearer {{token}}"]],
      "body": null,
      "name": "ok",
      "skip": false,
      "assertions": ["status == 200"],
      "captures": [],
      "source_line": 3
    }
  ]
}
```

字段集以现场 `ParsedRequest`/`RequestMetadata` 为准锁定 v1.0（`captures` v1 输出原始声明文本）；`FileMetadata` 等文件级规则字段登记后补，不在 v1 强求。

**先写测试（Red）：**

1. e2e `tests/ast_command_test.rs`（运行期红：`ast` 子命令不存在 → clap 未知命令 exit 2 → 断言失败）：

```rust
//! WP-AST 守护：`rupost ast` 输出稳定 JSON 契约；错误路径诚实失败。
//! 反假守护：改 AstDocument 顶层字段名 → schema 断言红；删除投影回填 → 字段断言红。
mod common;

use std::process::Command;
use tempfile::tempdir;

fn run(dir: &std::path::Path, history: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rupost"))
        .env("RUPOST_HISTORY_DIR", history)
        .current_dir(dir)
        .args(args)
        .output()
        .expect("spawn rupost")
}

#[test]
fn test_ast_json_contract() {
    let dir = tempdir().unwrap();
    let history = tempdir().unwrap();
    // 行号台账：1 ### / 2 assert / 3 GET
    std::fs::write(
        dir.path().join("suite.http"),
        "### ok\n# @assert status == 200\nGET {{base_url}}/ok\n",
    )
    .unwrap();

    let out = run(dir.path(), history.path(), &["ast", "suite.http"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("ast 输出必须为纯 JSON: {stdout}");

    // 顶层契约：字段集精确（4 键）
    let obj = v.as_object().unwrap();
    assert_eq!(obj.len(), 4, "{obj:?}");
    for k in ["schema_version", "source_path", "dependencies", "requests"] {
        assert!(obj.contains_key(k), "missing: {k}");
    }
    assert_eq!(v["schema_version"], "1.0");

    // 请求级契约
    let req = &v["requests"][0];
    assert_eq!(req["number"], 1);
    assert_eq!(req["method"], "GET");
    assert_eq!(req["url"], "{{base_url}}/ok");
    assert_eq!(req["name"], "ok");
    assert_eq!(req["skip"], false);
    assert_eq!(req["assertions"][0], "status == 200");
    assert_eq!(req["source_line"], 3);
}

#[test]
fn test_ast_missing_file_fails_honestly() {
    let dir = tempdir().unwrap();
    let history = tempdir().unwrap();
    let out = run(dir.path(), history.path(), &["ast", "nope.http"]);
    assert_ne!(out.status.code(), Some(0), "缺失文件必须非零");
}
```

2. 投影层单测（`src/parser/ast_json.rs` 的 `#[cfg(test)] mod tests`）：顶层与请求级键集精确断言（同 e2e）+ `headers` 保序断言（双头文档）。

**实施步骤（Green）：**

1. `src/parser/ast_json.rs`：`pub struct AstDocument`/`AstRequest`（`#[derive(Serialize)]`，字段名即契约）+ `pub fn to_ast_document(parsed: &ParsedFile) -> AstDocument`；`rg "ParsedRequest {" src/parser/` 对照字段来源逐个回填；`captures` 输出原始声明文本。
2. `src/cli/commands/mod.rs`：`Commands::Ast(AstArgs)`（`file` 位置参，`value_name = "FILE"`）——加入 `plugin_route.rs:376-384` 的内置子命令名单（防插件抢占 `ast` 名）。
3. `src/cli/dispatch.rs`：分发臂——`std::fs::read_to_string` → `parse_document_by_path` → `to_ast_document` → `serde_json::to_vec_pretty` → stdout；文件缺失/解析错误走既有错误链（非零退出）。
4. `src/cli/plugin_exec.rs`：白名单重建处（:195-211 附近）追加 `cmd.env("RUPOST_BIN", std::env::current_exe().map(|p| p).unwrap_or_else(|_| "rupost".into()));`（取不到 exe 时退化为 PATH 名，不阻断）。
5. 作者指南补节：按 [插件承载能力评估](2026-11-13-plugin-readiness-assessment.md) §7/§8 转写"AST 桥使用方案（端到端时序、字段使用手册、k6gen 最小示例、陷阱清单）"与"版本协调（三版本轴、核心承诺、作者六条守则、错位场景）"两节——内容以该评估报告为 SSOT，不得另行发挥。

**必须避免：** 不做解析器类型直接 derive；stdout 不打任何日志/噪音（日志走 stderr，与 `--report json` 同纪律）；不给 ast 命令加输出文件旗标（管道组合即可）；不改 hook 协议。

**验证与提交：**

```bash
cargo test --lib parser::ast_json
cargo test --test ast_command_test
cargo test --test cli_plugin_route_test      # RUPOST_BIN 注入断言
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
jj split src/parser src/cli tests/ast_command_test.rs doc/architecture/features/plugin_and_extensibility.md -m "feat(parser,cli): expose document ast json (rupost ast) + RUPOST_BIN for plugins"
```



### WP-P2b（Phase 2②）：契约漂移基线门禁

**目的：** `--contract-baseline <FILE>`：首跑创建基线；复跑对比漂移（newly-broken/fixed/new）打 stderr 摘要并更新基线。只记录确定性字段。

**文件：**

- 新增 `src/report/contract.rs`（含表驱动单测）
- 修改 `src/report/mod.rs`（`pub mod contract;` + re-export）
- 修改 `src/cli/commands/test.rs`（`--contract-baseline`）、`src/runner/app.rs`（Options 字段 + Default）、`src/cli/dispatch.rs`（映射）、`src/cli/suite_presenter.rs`（漂移块）、`tests/html_report_integration_test.rs`（构造点迁移——该文件为显式字段字面量）
- 新增 `tests/contract_drift_test.rs`（e2e）

**领域模块骨架（照抄后按现场微调导入）：**

```rust
//! 契约漂移基线（WP-P2b）：只记录确定性字段（断言原文 + 通过与否），动态值
//! 从根上不进摘要；diff 以 `id::assertion` 为键。
use crate::runner::types::TestResult;
use crate::utils::path_guard::normalize_posix_path;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

pub const CONTRACT_SCHEMA_VERSION: &str = "1.0";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContractEntry {
    pub id: String,
    pub assertion: String,
    pub passed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ContractSummary {
    pub schema_version: String,
    pub contracts: Vec<ContractEntry>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContractDrift {
    pub newly_broken: usize,
    pub fixed: usize,
    pub new: usize,
}

/// 稳定 ID：<path>#L<行号>；无行号退化 <path>#<序号>
pub fn entry_id(path: &std::path::Path, result: &TestResult) -> String {
    match result.source_line {
        Some(line) => format!("{}#L{line}", normalize_posix_path(path)),
        None => format!("{}#{}", normalize_posix_path(path), result.request_number),
    }
}

pub fn build_contract_summary(
    batch_results: &[(std::path::PathBuf, Vec<TestResult>)],
) -> ContractSummary {
    let mut contracts = Vec::new();
    for (path, results) in batch_results {
        for r in results {
            if r.is_teardown {
                continue;
            }
            for a in &r.assertions {
                contracts.push(ContractEntry {
                    id: entry_id(path, r),
                    assertion: a.raw.clone(),
                    passed: a.passed,
                });
            }
        }
    }
    ContractSummary {
        schema_version: CONTRACT_SCHEMA_VERSION.to_string(),
        contracts,
    }
}

/// diff：键 = id::assertion（断言原文改变视同新增）
pub fn diff_contracts(baseline: &ContractSummary, current: &ContractSummary) -> ContractDrift {
    let key = |e: &ContractEntry| format!("{}::{}", e.id, e.assertion);
    let base: HashMap<String, bool> =
        baseline.contracts.iter().map(|e| (key(e), e.passed)).collect();
    let mut drift = ContractDrift::default();
    for e in &current.contracts {
        match base.get(&key(e)) {
            Some(true) if !e.passed => drift.newly_broken += 1,
            Some(false) if e.passed => drift.fixed += 1,
            None => drift.new += 1,
            _ => {}
        }
    }
    drift
}

pub fn save_baseline(path: &Path, summary: &ContractSummary) -> crate::Result<()> {
    let bytes = serde_json::to_vec_pretty(summary)?;
    crate::security::fs::write_private_file_atomic(path, &bytes)
}

pub fn load_baseline(path: &Path) -> crate::Result<Option<ContractSummary>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(crate::error::RupostError::IoError(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, assertion: &str, passed: bool) -> ContractEntry {
        ContractEntry { id: id.into(), assertion: assertion.into(), passed }
    }

    /// 表驱动：broken / fixed / new / unchanged 四类漂移
    #[test]
    fn test_diff_contracts_table() {
        let mk = |items: &[(&str, &str, bool)]| ContractSummary {
            schema_version: CONTRACT_SCHEMA_VERSION.into(),
            contracts: items.iter().map(|(i, a, p)| entry(i, a, *p)).collect(),
        };
        let cases = [
            (mk(&[("a#L1", "status == 200", true)]), mk(&[("a#L1", "status == 200", false)]), (1, 0, 0), "newly broken"),
            (mk(&[("a#L1", "status == 200", false)]), mk(&[("a#L1", "status == 200", true)]), (0, 1, 0), "fixed"),
            (mk(&[]), mk(&[("a#L1", "status == 200", true)]), (0, 0, 1), "new"),
            (mk(&[("a#L1", "status == 200", true), ("b#L2", "status == 404", false)]), mk(&[("a#L1", "status == 200", true), ("b#L2", "status == 404", false)]), (0, 0, 0), "unchanged"),
            // 断言原文改变视同新增
            (mk(&[("a#L1", "status == 200", true)]), mk(&[("a#L1", "status == 201", true)]), (0, 0, 1), "assertion text changed counts as new"),
        ];
        for (base, current, expected, name) in cases {
            assert_eq!(diff_contracts(&base, &current).newly_broken, expected.0, "{name}");
            assert_eq!(diff_contracts(&base, &current).fixed, expected.1, "{name}");
            assert_eq!(diff_contracts(&base, &current).new, expected.2, "{name}");
        }
    }

    /// schema 精确集：顶层两键、条目三键、无 actual 值
    #[test]
    fn test_summary_schema_exact_keys() {
        let summary = ContractSummary {
            schema_version: CONTRACT_SCHEMA_VERSION.into(),
            contracts: vec![entry("ok.http#L3", "status == 200", true)],
        };
        let v: serde_json::Value = serde_json::to_value(&summary).unwrap();
        let obj = v.as_object().unwrap();
        assert_eq!(obj.len(), 2);
        assert!(obj.contains_key("schema_version") && obj.contains_key("contracts"));
        assert_eq!(obj["contracts"][0].as_object().unwrap().len(), 3);
        assert_eq!(v["schema_version"], "1.0");
    }

    #[test]
    fn test_entry_id_line_and_fallback() {
        let mut r = TestResult::error(1, None, "GET".into(), "http://x".into(), "e".into(), std::time::Duration::from_millis(1));
        let id = entry_id(std::path::Path::new("a.http"), &r);
        assert_eq!(id, "a.http#1", "无行号退化序号");
        r.source_line = Some(7);
        assert_eq!(entry_id(std::path::Path::new("a.http"), &r), "a.http#L7");
    }
}
```

`src/report/mod.rs`：追加 `pub mod contract;` 与 `pub use contract::{ContractDrift, ContractEntry, ContractSummary, CONTRACT_SCHEMA_VERSION, build_contract_summary, diff_contracts, load_baseline, save_baseline};`

**CLI/e2e：**

1. `TestArgs`（cli/commands/test.rs，放在 `--incident-out` 之后）：

```rust
    /// WP-P2b 契约漂移基线：首跑创建基线；后续运行对比漂移（stderr 摘要）并更新。
    /// 只记录确定性字段：断言原文与通过与否，不记录响应值；漂移不改变退出码。
    #[arg(long, value_name = "FILE")]
    pub contract_baseline: Option<String>,
```

2. `TestSuiteOptions` 加 `pub contract_baseline: Option<String>`（Default None）；`rg "TestSuiteOptions {"` 迁移清单（dispatch.rs:68 映射 + html_report_integration_test.rs 两处显式字面量补 `contract_baseline: None,`）。
3. `present_suite_outcome`：报告文件落盘之后、incident 块**之前**插入：

```rust
    // WP-P2b：契约漂移基线（只记确定性字段；漂移不改变退出码）
    if let Some(baseline_path) = &options.contract_baseline {
        let summary = rupost::report::contract::build_contract_summary(&outcome.batch_results);
        let baseline =
            rupost::report::contract::load_baseline(std::path::Path::new(baseline_path))?;
        match &baseline {
            Some(base) => {
                let drift = rupost::report::contract::diff_contracts(base, &summary);
                if drift.newly_broken > 0 {
                    eprintln!("⚠ 契约漂移: {} 条契约相较基线由通过转为失败", drift.newly_broken);
                }
                if drift.fixed > 0 {
                    eprintln!("[i] {} 条契约相较基线修复", drift.fixed);
                }
                eprintln!("[i] {} 条新增契约已写入基线", drift.new);
            }
            None => {
                eprintln!(
                    "[i] 契约基线已创建: {baseline_path}（共 {} 条；下次运行将报告漂移）",
                    summary.contracts.len()
                );
            }
        }
        rupost::report::contract::save_baseline(std::path::Path::new(baseline_path), &summary)?;
    }
```

4. e2e `tests/contract_drift_test.rs`（完整照抄；反假守护：把 `diff_contracts` 临时改为恒返回 `ContractDrift::default()`，第二段"契约漂移"断言必须变红）：

```rust
//! WP-P2b 守护：--contract-baseline 首跑建基线、复跑报漂移并更新基线；
//! 基线文件 schema 精确（无 actual 值）。
mod common;

use std::process::Command;
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn run(dir: &std::path::Path, history: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rupost"))
        .env("RUPOST_HISTORY_DIR", history)
        .current_dir(dir)
        .args(args)
        .output()
        .expect("spawn rupost")
}

#[tokio::test]
async fn test_contract_baseline_creates_then_reports_drift() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ok"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let dir = tempdir().unwrap();
    let history = tempdir().unwrap();
    // 行号台账：1 ### / 2 assert / 3 GET（id 稳定为 ok.http#L3）
    std::fs::write(
        dir.path().join("ok.http"),
        format!("### ok\n# @assert status == 200\nGET {}/ok\n", server.uri()),
    )
    .unwrap();

    // 首跑：创建基线
    let out = run(dir.path(), history.path(), &["t", "ok.http", "--contract-baseline", "base.json"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("契约基线已创建"), "{stderr}");

    // 基线 schema 精确：顶层两键、条目三键、无 actual
    let raw = std::fs::read(dir.path().join("base.json")).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    let obj = v.as_object().unwrap();
    assert_eq!(obj.len(), 2, "{obj:?}");
    assert!(obj.contains_key("schema_version") && obj.contains_key("contracts"));
    assert_eq!(v["schema_version"], "1.0");
    let entry = v["contracts"][0].as_object().unwrap();
    assert_eq!(entry.len(), 3);
    assert!(entry.contains_key("id") && entry.contains_key("assertion") && entry.contains_key("passed"));
    assert_eq!(v["contracts"][0]["id"], "ok.http#L3");
    assert_eq!(v["contracts"][0]["passed"], true);

    // 复跑：同文件名重写为 500 端点（id 保持 ok.http#L3 不变）→ 漂移可见 + 基线更新
    let server2 = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ok"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server2)
        .await;
    std::fs::write(
        dir.path().join("ok.http"),
        format!("### ok\n# @assert status == 200\nGET {}/ok\n", server2.uri()),
    )
    .unwrap();
    let out = run(dir.path(), history.path(), &["t", "ok.http", "--contract-baseline", "base.json"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("契约漂移"), "复跑必须报告漂移: {stderr}");
    let raw = std::fs::read(dir.path().join("base.json")).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(v["contracts"][0]["passed"], false, "基线必须随复跑更新");
}
```

**必须避免：** 摘要文件不写任何 actual/响应值/URL（id 中只有路径与行号）；漂移不 `process::exit`；基线覆写不做"目标必须不存在"检查（基线语义就是覆写更新，但必须走 0600 原子写）；不动退出码。

**验证与提交：**

```bash
cargo test --lib report::contract
cargo test --test contract_drift_test
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
jj split src/report src/cli src/runner tests/contract_drift_test.rs tests/html_report_integration_test.rs -m "feat(report,cli): contract drift baseline gate (--contract-baseline)"
```

### WP-P3a（Phase 3①）：OpenAPI(JSON) 导入

**目的：** `rupost import openapi <spec.json> -o out.md`：把 JSON spec 转成单一可审查 Markdown（复用既有 `ImportFormat::OpenApi` 检测；实现后 web `/api/v1/import` 免费获得）。

**文件：**

- 新增 `src/importer/openapi.rs`（含内容契约单测）
- 修改 `src/importer/mod.rs`（分发加显式 `ImportFormat::OpenApi` 臂；**自动检测逻辑已存在勿改**）
- 修改 `src/cli/commands/import.rs`（`ImportCommands::OpenApi`）、`src/cli/dispatch.rs`（分发分支）
- e2e（新建 `tests/openapi_import_test.rs`）

**现场核验记录（2026-11-13 审查，实测当前 HEAD）：**

- `ImportRequest` 实际字段 = `format / content / templatize_host / generate_mock`（**无 output**；输出落盘由 CLI 参数层处理）——importer/mod.rs:43-51；
- `ImportResponse` 实际字段 = `detected_format / markdown_blocks: Vec<String> / total_imported / warnings`（**无 content**）——importer/mod.rs:55-60；
- `Har/OpenApi/Postman` 现状落入 `other =>` 回退**按 RawHttp 尝试**（mod.rs:127-133）——本 WP 增加显式 `ImportFormat::OpenApi` 臂替换之；
- 其他 importer 的实现形状 = `parse(&self, content, templatize_host) -> Result<ImportResponse>`（curl.rs/httpie.rs 同款）；`-o` 防覆盖在 CLI 参数层（`ImportCurlArgs.output`，已存在拒绝）——import.rs:14-45。

**先写测试（Red：`openapi` 模块不存在，编译失败即红证据）。放入 `src/importer/openapi.rs` 的 `#[cfg(test)] mod tests`：**

```rust
/// WP-P3a：OpenAPI(JSON) → 可审查 Markdown 的内容契约（contains 断言，非全文相等）。
/// 反假守护：临时删除分发中的显式 OpenApi 臂（回退 RawHttp），本测试必须稳定变红。
#[test]
fn test_openapi_import_content_contract() {
    let spec = r#"{
      "openapi": "3.0.0",
      "info": { "title": "Demo API" },
      "paths": {
        "/users/{id}": {
          "get": {
            "operationId": "getUser",
            "parameters": [
              { "name": "id", "in": "path", "required": true,
                "schema": { "type": "string", "example": "u1" } },
              { "name": "verbose", "in": "query", "schema": { "type": "boolean" } }
            ],
            "responses": { "200": { "description": "ok" } }
          }
        },
        "/tokens": {
          "post": {
            "requestBody": { "content": { "application/json": { "schema": { "type": "object",
              "properties": { "user": { "type": "string", "example": "admin" } } } } } },
            "responses": { "201": { "description": "created" } }
          }
        }
      },
      "components": { "securitySchemes": { "bearer": { "type": "http", "scheme": "bearer" } } },
      "security": [ { "bearer": [] } ]
    }"#;
    let request = crate::importer::ImportRequest {
        format: crate::importer::ImportFormat::OpenApi,
        content: spec.to_string(),
        templatize_host: false,
        generate_mock: false,
    }; // 字段集实测：format/content/templatize_host/generate_mock（无 output）
    let resp = crate::importer::MultiFormatImporter.import(&request);
    let md = resp.markdown_blocks.join("\n"); // 生成物是 markdown_blocks 向量（无 content 字段）
    assert!(md.contains("# @name getUser"), "operationId → @name: {md}");
    assert!(md.contains("{{base_url}}/users/{{id}}"), "路径参数必须模板化: {md}");
    assert!(md.contains("Authorization: Bearer {{token}}"), "bearer 必须模板化: {md}");
    assert!(md.contains("# @assert status == 200"), "{md}");
    assert!(md.contains("# @assert status == 201"), "{md}");
    assert!(md.contains("\"user\": \"admin\""), "body example 必须落地: {md}");
    assert!(!md.contains("verbose"), "可选 query 缺省省略: {md}");
}
```

（`MultiFormatImporter.import` 的返回形态以现场为准——若为 `Result` 按现场加 unwrap；**内容断言契约不得改**。）

**实施步骤（Green）：**

1. `src/importer/openapi.rs`：`OpenApiImporter`，`parse(&self, content: &str, templatize_host: bool) -> Result<ImportResponse>`（与 curl/httpie 同款签名）；serde_json::Value 导航（不建 typed 树）；生成规则照 §2.5；**仅 JSON**——`serde_json::from_str::<Value>` 失败即返回可读错误（"本批仅支持 JSON spec，YAML 登记未做"）。
2. `src/importer/mod.rs`：match 增加显式臂 `ImportFormat::OpenApi => OpenApiImporter.parse(&req.content, req.templatize_host)?,`（把 OpenApi 从 `other =>` 回退中摘出）。
3. CLI：`ImportCommands::OpenApi(ImportOpenApiArgs)`——`spec: String`（spec **文件路径**）、`output: Option<String>`（已存在拒绝，对齐 ImportCurlArgs 语义）、`templatize_host: bool`；dispatch 读 spec 文件 → `ImportRequest { format: OpenApi, .. }` → import → `markdown_blocks.join("\n")` 写 output / 缺省 stdout。
4. e2e `tests/openapi_import_test.rs`（纯 `#[test]` 子进程，无网络）：spec.json → `import openapi spec.json -o out.md` success + 内容标记断言；rerun 同命令 → 防覆盖非零且 out.md 不变。

**必须避免：** 不新增依赖（禁 serde_yaml）；不动自动检测逻辑；生成物必须是纯 Markdown 文本；不做请求体 schema 深度合成。

**验证与提交：**

```bash
cargo test --lib importer::openapi
cargo test --test openapi_import_test
cargo test --test web_api_integration_test   # web import 自动识别回归
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
jj split src/importer src/cli tests/openapi_import_test.rs -m "feat(importer,cli): openapi json spec import to reviewable markdown"
```

### WP-P3b（Phase 3②）：`export k6` 场景草稿导出

**目的：** `rupost export k6 <file> -o script.js`：文档 → 可运行的 k6 脚本骨架（与 curl/httpie/wget 同 UX、同旗标）。

**现场核验记录（2026-11-13 审查，三处架构事实修正原骨架）：**

- export 管线是**逐请求** `CommandExporter::export_one(&ExportableRequest, &ExportOptions) -> Result<String>`（exporter/plugin.rs:19），**文件级包装（imports/options）没有挂点** → trait 加默认方法 `file_preamble() -> Option<String> { None }`（OCP：curl/httpie/wget 零改动，k6 覆写）；
- 管线经 `RequestPreparer` **变量已插值**（mod.rs:110 起 var_context 装配，ExportableRequest.url 是解析后绝对 URL）→ `{{var}} → __ENV.VAR` 映射**不成立**，凭据纪律 = **`--redact`**（既有脱敏管线在 export 内生效：`redact_secrets: opts.redact`）；
- `ExportableRequest`（types.rs:7-26）**不含断言** → v1 草稿**不含 `check()`**，status 期望以注释承载（登记：断言进导出管线不做）；`@skip`/teardown 已被 preparer 排除（白得）；`source_location`（"file:line"）现成可作注释。

**文件：** `src/exporter/plugin.rs`（trait 默认方法）、`src/exporter/mod.rs`（preamble 装配一次）、新增 `src/exporter/plugins/k6.rs`、`new_builtin()` 注册、e2e `tests/k6_export_test.rs`。

**先写测试（Red：模块不存在编译失败即红证据）。`src/exporter/plugins/k6.rs` 的 `#[cfg(test)] mod tests`：**

```rust
/// WP-P3b：k6 场景草稿内容契约（contains 断言）。
/// 反假守护：临时让 file_preamble 返回 None，imports 断言必须稳定变红。
#[test]
fn test_k6_fragment_and_preamble_contract() {
    use crate::exporter::types::{ExportOptions, ExportableRequest};
    let req = ExportableRequest {
        name: Some("ok".into()),
        source_location: Some("s.http:3".into()),
        method: "GET".into(),
        url: "http://127.0.0.1:8080/ok".into(),
        headers: vec![("Authorization".into(), "[REDACTED]".into())],
        body: None,
        timeout_secs: None,
        is_sse: false,
        warnings: Vec::new(),
    };
    let opts = ExportOptions { compact: false, redact_secrets: true };
    let k6 = super::K6Exporter;
    let frag = k6.export_one(&req, &opts).unwrap();
    assert!(frag.contains("http.get(\"http://127.0.0.1:8080/ok\")"), "{frag}");
    assert!(frag.contains("// s.http:3"), "source_location 注释: {frag}");

    let pre = k6.file_preamble().unwrap();
    assert!(pre.contains("import http from 'k6/http'"), "{pre}");
    assert!(pre.contains("vus: 1") && pre.contains("iterations: 1"), "负载模型必须为 stub: {pre}");
}
```

（ExportableRequest/ExportOptions 字段以现场定义为准；断言契约不得改。）

**实施步骤（Green）：**

1. `src/exporter/plugin.rs`：trait 追加默认方法 `/// 文件级前导（imports/options 等脚本包装）；默认 None 不影响既有 target（OCP）` `fn file_preamble(&self) -> Option<String> { None }`。
2. 新增 `src/exporter/plugins/k6.rs`：`K6Exporter`——`name()=="k6"`、`description()`；`export_one` 返回单请求片段：注释 `// {source_location} {name}` + `http.<verb>(...)`（headers/body 进 params）；`file_preamble` 返回 imports + `options = { vus: 1, iterations: 1 }`（stub 注释"负载模型自行扩展"）+ `BASE_URL` 提示注释。
3. `src/exporter/mod.rs`：`run_export` 逐请求装配前（`rendered_commands` 建立后）`if let Some(pre) = exporter.file_preamble() { rendered_commands.push(pre) }`——仅一次；curl/httpie/wget 返回 None 零影响。
4. `new_builtin()` 注册 `K6Exporter`（`--list` 自动收录）。
5. e2e `tests/k6_export_test.rs`（纯 `#[test]` 子进程）：写含 `Authorization: Bearer real-token-xyz` 的 suite.http → `export k6 suite.http -o out.js --redact` → success；out.js 含 `import http from 'k6/http'`；**不含 `real-token-xyz`**（--redact 生效）；rerun 无 `--force` → 防覆盖非零；`--force` → success。

**必须避免：** 不生成 vus/stages/thresholds 数值模型（stub 注释除外）；不做断言翻译（断言不在导出管线——登记不做）；不新增依赖；`{{var}} → __ENV.VAR` 映射随插值语义一并取消（勿 resurrect）。

**验证与提交：**

```bash
cargo test --lib exporter::plugins::k6
cargo test --test k6_export_test
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
jj split src/exporter tests/k6_export_test.rs -m "feat(exporter): k6 scenario draft export (export k6 <file>)"
```

### WP-R5（缺口 5 收尾）：run_all.sh 端口平移与健康探针

**目的：** examples 回归脚本对齐 `verify_features.sh` 的可信度。incident 段已由 `d8913f0f` 先行完成 env 端口（`RUPOST_INCIDENT_MOCK_PORT`）+ 有限重试探针；本批补齐 llm/sse/plugin 段。

**文件：** 只改 `examples/run_all.sh`。

**实施步骤：**

1. 顶部（色彩定义后）加端口基址：
```bash
EXAMPLE_PORT_BASE="${RUPOST_EXAMPLE_PORT_BASE:-8080}"
LLM_PORT=$((EXAMPLE_PORT_BASE))
PLUGIN_PORT=$((EXAMPLE_PORT_BASE + 10))
```
2. LLM/SSE 段：两处 `--port 8080` → `--port "$LLM_PORT"`；三条 `run_case` 的 `--var base_url=http://127.0.0.1:8080` → `http://127.0.0.1:$LLM_PORT`；两处 `sleep 2` 换探针。llm_demo 与 sse_demo 共用同一端口顺序复用——保持同端口不变。
3. Plugin 段：`--port 8090` → `--port "$PLUGIN_PORT"`；两条 `run_case` 同理；`sleep 2` 换探针。
4. 顶部函数区提炼探针（与 `verify_features.sh` 的 `wait_mock_ready` 同款）：
```bash
wait_port_ready() {
    local port=$1 log=$2 pid=$3 retries=50
    while [ "$retries" -gt 0 ]; do
        if ! kill -0 "$pid" 2>/dev/null; then
            echo -e "${RED}[ERROR] Mock 启动即退出（端口 ${port} 被占用或配置非法），日志：${NC}"
            cat "$log" 2>/dev/null || true; exit 1
        fi
        if curl -s -o /dev/null "http://127.0.0.1:${port}/"; then return 0; fi
        sleep 0.2; retries=$((retries - 1))
    done
    echo -e "${RED}[ERROR] Mock 端口 ${port} 未在超时内就绪，日志：${NC}"
    cat "$log" 2>/dev/null || true; exit 1
}
```
   incident 段的内联探针可顺带换用此函数（可选，不强制）。

**必须避免：** 不动外网用例（直连公共测试源的免配置示例）；不合并 `RUPOST_TEST_PORT_BASE`（两脚本各管各）；默认端口行为必须与历史完全一致。

**验证：** `bash -n examples/run_all.sh`；`./examples/run_all.sh` 全 PASS；`RUPOST_EXAMPLE_PORT_BASE=18080 ./examples/run_all.sh` 再跑全 PASS（偏移真实生效，A-R6）。

**提交：** `test(examples): make example runner ports portable and probe-gated`

### WP-R6（缺口 6）：manifest 完整性摘要 `artifact_digests`（digests，非签名）

**目的：** 复现包接收方可校验工件完整性。命名与边界见 §2.6（digest 只防损坏，防恶意篡改的签名登记不做）。

**文件：** `src/history/incident.rs`、`tests/incident_bundle_test.rs`。

**实施步骤：**

1. 类型（schema 只追加）：
```rust
/// 工件完整性摘要（WP-R6）：只防传输损坏/无意篡改；防恶意篡改需密钥签名（登记不做）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactDigest {
    pub file: String,
    /// 64 位小写 hex sha256（编码写法复用 snapshot_io.rs 的 format!("{b:02x}") 模式，不新增 hex 依赖）
    pub sha256: String,
    pub bytes: u64,
}
```
   `IncidentManifest` 追加 `#[serde(default)] pub artifact_digests: Vec<ArtifactDigest>`。
2. `create_incident_bundle` 内，report 与 snapshot 落盘之后、manifest 序列化之前：
```rust
let mut digests = Vec::new();
for name in [IncidentArtifactNames::REPORT, IncidentArtifactNames::SNAPSHOT] {
    let bytes = std::fs::read(staging.join(name))?;
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    digests.push(ArtifactDigest {
        file: name.to_string(),
        sha256: hasher.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>(),
        bytes: bytes.len() as u64,
    });
}
```
   manifest 的 `artifact_digests: digests`。manifest 自身不入表（自引用不可能；消费方语义：两工件摘要匹配 + manifest 存在即完整）。
3. `test_manifest_schema_fields_exact_contract` 的 `expected_keys` 追加 `"artifact_digests"`（10→11 键，只追加合规）。

**先写测试（Red 可编译：字段不存在时 JSON 键缺失 → 断言失败）：**

- 扩展 `test_create_incident_bundle_end_to_end`：digests 恰含两文件、sha256 为 64 位小写 hex、与测试内独立计算（测试自己 `sha2` 哈希 report/snapshot 字节）一致；`manifest.json` 不在表中。
- CLI e2e（`test_failed_run_emits_redacted_replayable_incident_bundle`）追加：`manifest["artifact_digests"]` 长度为 2，且独立哈希 `snapshot.json` 字节与表中条目相等。
- `test_manifest_schema_fields_exact_contract` 断言键集同步。

**反假守护：** 临时让 digests 返回空 vec → end-to-end 红 → 恢复。

**消费侧登记（不做）：** `replay`/`ai triage` 不强制校验摘要（只读兼容）；未来可选 `--verify-digests`。

**验证与提交：**

```bash
cargo test --lib history::incident
cargo test --test incident_bundle_test
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
jj split src/history/incident.rs tests/incident_bundle_test.rs -m "feat(history): add artifact integrity digests to incident manifest"
```

### WP-R7：Web 复现包保留上限（安全分析残余风险收口）

**目的：** 封堵残余风险——`--dev`/局域网下恶意页面反复"失败运行 + incident=true"无限建目录（低危磁盘 DoS）。**仅 Web 通道**；CLI 显式路径不做保留策略（§2.6）。

**文件：** `src/web/handlers/run.rs`（新私有 fn + 该文件首个 `mod tests`）、`tests/web_api_integration_test.rs`。

**实施步骤：**

1. 常量与判别（run.rs）：
```rust
/// Web 复现包保留上限：创建前裁剪最旧（安全分析残余收口；CLI 通道不适用）
const MAX_INCIDENT_BUNDLES: usize = 32;

/// 服务端命名判别：`YYYYMMDDTHHMMSS-8hex`（len==24）。只删匹配命名的目录，
/// 绝不触碰用户手工放入 incident_root 的内容；symlink/文件一律跳过。
fn is_server_named_incident(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() == 24
        && b[8] == b'T'
        && b[..8].iter().all(u8::is_ascii_digit)
        && b[9..15].iter().all(u8::is_ascii_digit)
        && b[15] == b'-'
        && b[16..24].iter().all(u8::is_ascii_hexdigit)
}

/// 匹配目录数 >= max 时按名称升序（即时间序）删除最旧
fn prune_incident_dirs(root: &std::path::Path, max: usize) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    let mut named: Vec<std::ffi::OsString> = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.metadata().map(|m| m.is_dir()).unwrap_or(false)   // DirEntry::metadata 不跟随 symlink
                && is_server_named_incident(&e.file_name().to_string_lossy())
        })
        .map(|e| e.file_name())
        .collect();
    named.sort();
    while named.len() >= max {
        let Some(oldest) = named.first().cloned() else { break };
        let _ = std::fs::remove_dir_all(root.join(&oldest));
        named.remove(0);
    }
}
```
2. 调用点：`run_suite_handler` 的 incident 分支内、`create_incident_bundle` 之前：`prune_incident_dirs(&state.incident_root, MAX_INCIDENT_BUNDLES);`
3. lib 单测（run.rs 新建 `mod tests`）：
   - `is_server_named_incident` 表驱动：`20260101T000000-00000000` ok；长度 23 / 小写 `t` / 尾段含 `g` / 全数字无 `-` → false。
   - `prune_incident_dirs`：TempDir 内造 3 个假命名目录 + 1 个 `keepme` 普通目录 → `prune(max=2)` → 最旧删除、`keepme` 存活。
4. Web e2e（红测可编译——实现前 prune 不存在，行为如旧 → 断言失败）：
   预创建 32 个递增命名假目录（如 `20260101T0000NN-000000NN`，目录内放 sentinel 文件）→ 一次真实失败 incident 运行 → 断言：最早假目录消失、其余 31 + 新 bundle 存活、非命名目录不被触碰（A-R8）。

**反假守护：** 注释掉 prune 调用 → e2e 红 → 恢复。

**验证与提交：**

```bash
cargo test --lib web::
cargo test --test web_api_integration_test
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
jj split src/web/handlers/run.rs tests/web_api_integration_test.rs -m "fix(web): cap workbench incident bundle retention (oldest-pruned)"
```

### WP-R8（前端开关转正，可裁剪）：工作台套件面板 incident 复选框

**前置阅读：** `src/web/assets/app.js` 的 `runSuite` 流程 + `tests/web/app_test.mjs` 头部 Micro-DOM 打桩说明（零 npm 依赖）。

**改动：** 套件面板加复选框「失败时生成复现包」→ `runSuite` payload 带 `incident: true`；响应 `incident_dir` 非空时渲染一条提示（**必须走既有 `escapeHtml`**，与前端注入防线一致）。

**测试：** `app_test.mjs` 按既有用例的打桩方式断言 payload 含 incident 标志与提示渲染；`make test-web` 绿。

**提交：** `feat(web): surface incident bundle toggle in workbench suite panel`
**优先级：** 批三内最低，可整 WP 裁剪，不做不算缺口。

### WP-R9：`directory_structure.md` 补缺失模块条目（纯文档）

**现状核查：** `doc/architecture/directory_structure.md` §3.3 未覆盖 `src/web`（且未见 `src/history`、`src/security` 完整条目——执行时先 `ls src/` 对照，缺哪些补哪些；`src/web` 必补）。

**内容：** web 条目写明"辅助执行 + 辅助创建的薄表现层"定位并指向 `plugin_and_extensibility.md` §6（四条扩展缝）；history 条目写明历史/工件层与"零 cli/web/tui 依赖"红线（含 `incident.rs`）；security 条目写明 `redact.rs`/`fs.rs` 两个 SSOT。

**验证：** `grep -c "src/web" doc/architecture/directory_structure.md` 非零。

**提交：** `docs(architecture): cover src/web and missing modules in directory structure`

### WP-R4（缺口 4 + 全批收尾）：exit 2 鉴别文档与登记（纯文档）

**目的：** exit 2 的三个来源可被人与脚本鉴别；缺口修复与 Phase 2/3 MVP 全部改动登记。

**文件：**

- 修改 `doc/guides/debug_guide.md`
- 修改 `examples/incident_bundle/README.md`
- 修改 `doc/guides/testing_guide.md`
- 修改 `README.md`
- 修改 `CHANGELOG.md`
- 修改 `doc/plans/progress_summary.md`
- 修改本计划文档（状态行）

**内容要点（照此写入）：**

1. `debug_guide.md` §7.3 现有"退出码契约"一句扩充为鉴别表：

```markdown
**exit 2 鉴别表**（三个来源共享退出码 2；程序化区分靠 stderr 前缀）：

| 来源 | stderr 形态 |
| :-- | :-- |
| clap 参数错误 | `error: unexpected argument ...`（以 `error: ` 开头） |
| AST/解析错误（ParseError） | `[ERROR] 解析错误: ...` |
| 复现包工件创建失败（IncidentBundleError） | `[ERROR] 故障复现包创建失败: ...` |

CI 判读规则：测试结果只看 0/1；exit 2 一律视为"配置/工具/工件 I/O 问题"——重跑无意义，先修命令行参数或检查目标路径。三值契约（0/1/2）为既定外部契约（01 §2.1），不扩展第四值。
```

2. `examples/incident_bundle/README.md`：在"安全语义"末尾追加同款鉴别表（精简三行版）。
3. `debug_guide.md` §7.3 再补一句 Web 入口说明：`Web 工作台：套件运行请求带 incident:true 时，失败同样产出复现包，位置由服务端决定（<工作区>/.rupost/incidents/），客户端不可指定路径。`
4. `testing_guide.md` §4.1 的 CI 段落补一行：`退出码 2 的三来源鉴别见 debug_guide §7.3。`
5. `CHANGELOG.md` [Unreleased] 增加合并 bullet（覆盖全批）：
   - **Fixed**：Web 会话 Cookie 滑动续期（活跃会话不再因 Max-Age 固定倒计时 30 分钟硬过期）；
   - **Added**：Web 工作台套件失败可产出复现包（`RunSuiteRequest.incident` 开关，服务端管辖路径 `.rupost/incidents/`，客户端零路径输入，masking=secrets 强制）；
   - **Added（Phase 2）**：请求稳定定位（`<path>#L<行号>`，终端 `path:line` + JSON `source_line`）与契约漂移基线门禁 `--contract-baseline`（首跑建基线、复跑报漂移；摘要只含确定性字段）；
   - **Added（Phase 3）**：`import openapi`（JSON spec → 可审查 Markdown，复用既有 OpenApi 格式检测）与 `export k6`（场景草稿骨架，负载模型 stub）；
   - **Added（AST 桥）**：`rupost ast`（AstDocument JSON v1.0 只读命令）+ 插件环境注入 `RUPOST_BIN`——插件命令复用核心解析器，消除第三方转换工具的解析分叉（插件生态使能器）；
   - **test/docs**：`--env`→manifest/snapshot 的 CLI e2e；exit 2 三来源鉴别表；
   - **Added（批三）**：`run_all.sh` 端口平移与健康探针（`RUPOST_EXAMPLE_PORT_BASE`）；复现包完整性摘要 `artifact_digests`（digest 防损坏，签名登记不做）；Web 复现包保留上限（最旧裁剪 32 个，仅 Web 通道）；工作台 incident 复选框（若 R8 未裁剪）。
6. `progress_summary.md` 进度概览表顶部追加一行（只记已完成事实：四缺口修复形态 + Phase 2/3 MVP 交付物文件清单 + 守护测试名 + 反假守护记录）。
7. `README.md` 核心特性列表追加三行（各一句、链接对应指南）：文档契约门禁（稳定定位 + `--contract-baseline` 漂移基线）、OpenAPI 导入（`import openapi`，JSON spec → 可审查 Markdown）、k6 场景草稿（`export k6`）。
8. `testing_guide.md`：新增"契约漂移门禁（`--contract-baseline`）"小节（两连跑用法 + stderr 语义）；§4.1 测试登记追加 `source_location_test.rs`、`contract_drift_test.rs`、`openapi_import_test.rs`、`k6_export_test.rs`、`ast_command_test.rs`。
9. 最后把本文件顶部 `**状态：**` 改为 `Completed (C-R0~C-R4 与 C-P2a/P2b、C-P3a/P3b 全过)`。

**验证与提交：**

```bash
git diff --check
jj split doc/guides examples/incident_bundle/README.md README.md CHANGELOG.md doc/plans/progress_summary.md doc/plans/2026-11-13-phase01-remediation-plan.md -m "docs: document contract gate, interop bridges and exit-2 disambiguation"
```

---

## 4. 接手纪律（给 flash 级别模型）

1. **严格一次一个 WP**，顺序 WP-R3 → WP-R1 → WP-R2 → WP-R4。开始前 `jj status` 必须只含本 WP 将改的文件；混入其他文件先 `jj split` 出去留在 `@`。
2. **先读再动**：本文件对应 WP 章节 + 目标源文件全文 + 目标测试全文。不顺手重构相邻代码、不改无关格式。
3. **Red→Green 纪律**：测试先写、先跑出红、保存失败输出到 `/tmp/wp-rN-red.txt`，再写实现；每个守护测试必须做**反假守护**（临时破坏生产代码 → 确定性变红 → 恢复 → 绿），把红/绿证据留在会话记录里。
4. **依赖方向**：`history` 不得依赖 `web/cli/tui`；`web → history` 允许；CLI 只解析与呈现。
5. **Schema 只追加**：新字段一律 `#[serde(default)]`；不改既有字段名与语义。
6. **测试过滤器陷阱**：`cargo test` 一次只能跟一个名字过滤器，且 lib 过滤器会误伤集成测试（显示 0 tests）。lib 单测带名字过滤、集成测试不带，分开跑。
7. **Windows 兼容**：新断言不依赖路径分隔符（用 `contains(".rupost/incidents")` 而非 `starts_with`）；symlink 类断言加 `#[cfg(unix)]`。
8. **提交前**：`cargo fmt --all -- --check` + clippy `-D warnings` 必须绿；`jj split <文件> -m "<提交信息>"` 后执行 `jj diff -r @- --stat` 核对提交范围无吸收。
9. **提交信息**（照抄，不得改写）：见各 WP 验证块；风格沿用 `type(scope): summary`。
10. **内部结构未知先发现**：每个 WP 给出的 rg 关键词（如 `rg "ParsedRequest {" src/parser/`、`rg -n '"wget"' src/exporter/`）必须先跑、读现场再动手；严禁凭想象改解析器/导出器公共签名；新增字段后先 `cargo check` 让编译器列出构造点遗漏，逐个补齐。
11. 完成一个 WP 才开始下一个；全结束后跑 §5 最终门禁。

## 5. 检查点与最终门禁

| 检查点 | 可交付 | 禁止进入下一步的条件 |
| --- | --- | --- |
| C-R0 | 本计划文档入库 | — |
| C-R3 | `--env` e2e 绿 + 反假守护红证据 | 无破坏验证记录 |
| C-R1 | 滑动续期绿 | 续发 Cookie 改变会话 ID / 执行前失败路径开始带 Cookie |
| C-R2 | Web 复现包绿 | 客户端可指定输出路径 / 绿跑产出目录 / masking 非 secrets / bundle 失败被静默吞掉 |
| C-P2a | 来源定位绿（解析层单测 + e2e） | `.md` 与 `.http` 任一路径未覆盖 / `report_schema_test` 未登记追加键 / 快照 schema 被改动 |
| C-AST | `rupost ast` JSON 契约绿（含 schema 键集锁） | 直接 derive 解析器类型（内部命名泄漏）/ stdout 出现非 JSON 噪音 / `RUPOST_BIN` 未注入或指向错误 |
| C-P2b | 契约漂移绿（单元表驱动 + e2e） | 基线文件含 actual/响应值 / 漂移改变退出码 / 基线写入不走 0600 原子写 |
| C-P3a | OpenAPI 导入绿（内容契约单测 + CLI e2e） | YAML 被实现 / 新增依赖 / 生成物为私有格式 / 防覆盖缺失 |
| C-P3b | k6 导出绿（内容契约单测 + CLI e2e） | 生成了 vus/stages/thresholds 数值模型 / 全量断言翻译 / 新增依赖 |
| C-R5 | run_all.sh 默认/偏移双端口场景全 PASS | 任一场景 FAIL 或脚本仍存在写死端口 |
| C-R6 | digests 绿（单测 + CLI e2e） | snapshot/report 任一未入表 / schema 契约键数未同步 / 文档用"签名"表述 |
| C-R7 | 保留上限绿（单测 + web e2e） | 误删非命名目录 / 触碰 symlink / CLI 通道被裁剪 |
| C-R8 | 前端开关绿（可裁剪，跳过则记录） | 提示未走 escapeHtml |
| C-R9 | 目录文档覆盖 src/web 等缺失模块 | 缺 src/web 条目 |
| C-R4 | 文档与登记完成 | 有代码行为未文档化 / 本批未入 CHANGELOG 与 progress_summary |

最终门禁（全部执行并报告结果，外网 examples 失败要记录命令与错误、不得标绿）：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
make test-web
./tests/verify_features.sh
./examples/run_all.sh
git diff --check
```
