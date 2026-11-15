# 重放与免重填收口底账（Replay & Context-Reuse Remediation Backlog）

- **文档代号**: `replay-reuse-backlog`
- **立项日期**: 2026-11-12
- **状态**: Execution Ready（P0–P4 六批 22 项，全部 🔧 代码待落地；随实施推进更新为 ✅）
- **bookmark**: `replay-reuse`（基于 trunk `woovlorl`）
- **上游底账**: [`p0-p1-remediation-plan.md`](./2026-11-10-cross-scenario-engine-and-plugins/p0-p1-remediation-plan.md)（M1–M7 与 P0/P1 已全表 ✅，本账不重复收录已完成项）

---

## 0. 主题命题与第一性原理

**重放 = 信息重放，比对只是副产品。** 用户的核心诉求不是"验证响应变没变"，而是：

> 登录过的会话、捕获的 token、填过的参数——**直接复用，不再重新执行获取流程**（避免重填用户登录与其他上下文信息）。

### 四条信息轨道现状（2026-11-12 专项审查结论）

| 轨道 | 形态 | 跨运行复用 | 核心断点 |
|---|---|---|---|
| cookie 文件 | 0600 cookie 罐 JSON（`--cookie-file`） | ✅ CLI/TUI 闭环完整 | Web 零延续；过期原样重放无提示；不覆盖 Bearer/token 型 |
| capture 变量 | 内存 HashMap（`variable/types.rs:7-12`） | ❌ 进程即蒸发 | 第二次运行必重跑登录；TUI 断言红即丢捕获 |
| history 字节 | 变量解析后完整请求（含字面 token） | ✅ replay / TUI V 键免登录 | 过期 401 裸报无引导；生成模板时刻意丢弃字面凭据 |
| 快照字节 | 完整请求/响应（Schema v1.0） | ✅ 同上 | 脱敏即毁重放（互斥无中间态） |

四轨道各自为政：格式互不相通、生命周期互不衔接、**"会话资产失效 → 引导重新获取"的闭环全库缺位**。

---

## 1. P0 · 免重填主线（最高用户价值）

| ID | 工作项 | 证据锚点 | 修复方向 | 验收标准 | 状态 |
|---|---|---|---|---|---|
| R-1 | ✅ 已落地（2026-11-13：`--with-deps` opt-in 旗标 + `WorkflowGraph::closure_execution_order` 祖先闭包拓扑序；传递依赖 e2e + 既有语义防假守护对照） | `src/runner/app.rs`、`src/runner/engine/workflow.rs` | selector 分支计算目标文件祖先闭包，按拓扑序先执行；`--with-deps` 显式开关（保护既有 `--only` 契约） | `--only @get_order --with-deps` 自动先跑登录文件，`{{auth_token}}` 有值；无依赖文件零行为变化 | ✅ |
| R-2 | ✅ 已落地（2026-11-13：`src/variable/var_state.rs`（0600 原子写 + 损坏可报告）+ `--var-state [FILE]`（默认 `.rupost/vars.json`）双向接线；回灌只补缺、显式值恒优先；TUI 落盘留批后续） | `src/variable/var_state.rs`、`src/runner/{app.rs,suite_setup.rs}` | 套件结束捕获变量落盘（0600 原子写），启动回灌最低优先级层 | 跨运行免重跑登录 e2e（`tests/var_state_test.rs`：删登录文件后凭回灌成功 + `--var` 优先级防假守护） | ✅ |
| R-3 | ✅ 已落地（2026-11-13：`handle_request_finished` 捕获合并移出 Ok 分支——响应到达即承接；lib 层守护 `test_capture_merge_survives_failed_assertions`） | `src/tui/state/stream.rs` | 断言红/失败分支不影响捕获变量承接 | 登录 200 但断言红，token 仍被下游复用 | ✅ |
| R-4 | Web 轨道会话/变量延续 | `web/handlers/run.rs:79-87`（每次 ephemeral） | cookie bridge + variables 挂 `AppState`，复用 TUI 机制 | Web 逐块调试带登录链文档可复用会话 | ✅ 已落地（2026-11-13：AppState 增 `cookie_bridge`/`run_variables` 会话资产（内存态，服务生命周期）；run_block/run_suite 双 handler 回灌（只补缺，extra_variables 恒优先）+ cookie 导入导出 + 变量增量合并（跳过 `__` 保留前缀）；e2e `tests/web_session_reuse_test.rs` 三用例（复用/无会话诚实失败/显式覆盖防假守护）） |
| R-5 | ✅ 已落地（2026-11-13：replay mismatch 分支 401 → "疑似会话过期"再获取指引（`test_replay_401_emits_session_expiry_hint`）+ 套件汇总 401 失败集中提示） | `src/runner/engine/replayer.rs`、`src/cli/suite_presenter.rs` | 两级提示（逐条 + 汇总） | 过期不再是一串裸 mismatch | ✅ |

## 2. P1 · 行为级正确性（错误行为/错误结论）

| ID | 工作项 | 证据锚点 | 修复方向 | 验收标准 | 状态 |
|---|---|---|---|---|---|
| B-1 | ✅ 已落地（2026-11-13：`src/runner/executor_factory.rs` 共享工厂（`discover_plugin_registry` 拆层 DRY + `build_interactive_executor[_with]`）；TUI/Web 三构建点收口；装配守护 e2e 含防假守护对照） | `src/runner/executor_factory.rs`、`src/runner/suite_setup.rs`、`src/tui/handlers/actions/execution.rs`、`src/web/handlers/run.rs` | 注册表发现拆层为独立工厂函数，TUI/Web 经共享工厂装配本机注册表（项目级默认隔离 + env 通道一致） | 本机已装插件在 TUI/Web 真实生效（`tests/interactive_executor_test.rs`） | ✅ |
| B-2 | replay 感知 masking / body_truncated | `replayer.rs:24,398-429`；`model.rs:459-480`（masking 被丢弃） | masked 快照 → 警告或严格拒绝；截断条目跳过 body diff 只锁 status/headers 并注明原因 | 两类系统性假失败消灭 | ✅ 已落地（2026-11-13：`SnapshotSuite.masking`/`ResponseSnapshot.body_truncated` 透传 + replay 两级消费——masked 警告"数据不完整"、截断条目跳过 body diff 注明原因；`tests/replay_contract_test.rs` 4 用例） |
| B-3 | 报告 degraded 呈现 | `src/report/` 全目录零 degraded 字段 | JUnit → `<skipped>` 或 failure 注记；JSON/HTML 同步 | `--strict-plugins` 红 job 与看板不再互相矛盾 | ✅ 已落地（2026-11-13：JUnit degraded→`<skipped message="plugin degraded...">`（断言通过非 failure）+ JSON 报告 `degraded` 字段（snapshot_adapter 按 DegradedPass 填充）；schema 精确字段测试同步演进；e2e `test_degraded_pass_visible_in_junit_and_json_reports`） |
| B-4 | "未安装"文案补指引 | `pipeline.rs:264-268` vs `:171-174` | 与 RuntimeMissing 三选一处方对齐 | 首跑失败消息含 `--allow-missing-plugins` 指引 | ✅ 已落地（2026-11-13：未安装 blocking 文案补三选一处方，与 runtime_missing 对齐；`test_missing_plugin_blocking_message_includes_prescription`） |
| B-5 | replay 输入防线 | `snapshot_legacy.rs:29`（`read_to_string` 全量） | 64MB 上限 + `ensure_compatible_schema()` 版本门（与 triage 对齐，`triage.rs:183-197`） | 超大/未来版本快照显式拒绝而非静默错读 | ✅ 已落地（2026-11-13：`MAX_REPLAY_SNAPSHOT_BYTES` 64MB 体积门 + schema 主版本门（2.0+ 显式拒绝防静默错读）+ legacy version≤2 门；replay_contract 2 用例） |
| B-6 | 黑匣子自足性补全 | `model.rs:292-298`（@timeout 丢失）；`snapshot_io.rs:91-100`（error 仅无响应时录） | 快照请求镜像补 `@timeout`；有响应但断言失败补 error Detail | 超时类现场可忠实重放；失败 kind/耗时可追溯 | ✅ 全项落地（2026-11-13：`SnapshotRequest.timeout_ms` 往返——重放复现原超时语义；2026-11-14：**error Detail 经用户裁决解除 deferred**——断言失败条目 error=Some(AssertionFailed)+失败摘要，三形态守护测试 `test_error_detail_covers_assertion_failures_three_forms`，02 附录二登记权威口径；消费端 replay 判据=response.is_none() 不受影响） |

## 3. P2 · 安全对齐

| ID | 工作项 | 证据锚点 | 修复方向 | 验收标准 | 状态 |
|---|---|---|---|---|---|
| S-1 | history.jsonl 0600 + 脱敏评估 | `history/storage.rs:70-91`（umask 明文 append） | 权限对齐快照 0600；默认脱敏评估约束：**模板级重放链路（bootstrap/generate）不受损**（其本不依赖字面凭据） | 共享账号堡垒机凭据外带面收窄；`ai bootstrap` 链路回归绿 | ✅ 已落地（2026-11-13：`ensure_private_file`——history.jsonl 首建即 0600、宽松旧文件每次 append 前收紧（幂等 stat）；`test_history_file_permissions_tightened` 双路径守护） |
| S-2 | 掩码后重算 body_sha256 | `snapshot_io.rs:70-89,170-172`（sha 在掩码前算） | 掩码模式下摘要对掩码后形态重算（或注明语义） | 掩码快照离线验真链路成立 | ✅ 已落地（2026-11-13：`mask_execution_snapshot_entry` 掩码后重算 `body_sha256`——摘要对文件内掩码后形态（离线可验真），原始字节验真在掩码模式下按 masking=secrets 声明降级；`test_masked_body_sha256_matches_masked_form`） |
| S-3 | 按值掩码能力 | `test.rs:67-69`（bool 开关）；`redact.rs:8-33`（键名启发式） | `--mask-secret-value <VAL>` 或从 env 名单生成掩码表 | 词表外自定义 header（如 `x-vault-namespace`）密钥不再明文进快照 | ✅ 已落地（2026-11-13：`--mask-secret-value <VAL>`（可多次）→ 值掩码独立于 `--mask-secrets` 生效（url/headers/body/error/warnings/assertions 全表面 → [REDACTED]），masking 标记联动 secrets；`test_secret_value_masking_hits_all_surfaces` + e2e `test_mask_secret_value_hits_custom_headers`（x-vault-ns 词表外 header）） |

## 4. P3 · 体验与生态

| ID | 工作项 | 证据锚点 | 修复方向 | 验收标准 | 状态 |
|---|---|---|---|---|---|
| E-1 | 插件权威文档重写 | `doc/architecture/features/plugin_and_extensibility.md:83`（仍是 Wasm 旧设计） | 对齐 Stdio IPC/JSON manifest 现实 + README 插件章节 | 插件作者有唯一可信现行文档 | ✅ 已落地（2026-11-13：`doc/architecture/features/plugin_and_extensibility.md` 全面重写——废弃 Wasm/plugin.toml 旧设计声明 + Stdio IPC 现架构摘要 + 作者五步指南 + 安全不变量表 + 不做项登记） |
| E-2 | `plugin schema` 增强 | `plugin_route.rs:255-271`（仅 hooks 参数平铺） | 补 `@` 指令枚举 / 内置宏（`$sha256` 族）/ manifest schema / VSCode 挂接最小示例 | IDE 补全端到端可演示（场景 A 兑现） | ✅ 已落地（2026-11-13：`plugin schema` 四段输出——pluginDirectives（参数+阶段序+?optional）/ directiveSyntax 枚举 / builtinMacros（$hmac_sha256 等全宏表）/ manifestSchema（runtime/commands/hooks/保留字枚举）——场景 A 的 IDE 补全原料齐备） |
| E-3 | generate/bootstrap 凭据回填增强 | `generator/http.rs:171-180,78-84`（T2 不命中即弃值） | `--keep-credentials`（0600 本机回放）或缺失变量交互式写入 .env | 免去"翻 history 抄 token 填 .env" | ✅ 已落地（2026-11-13：`--keep-credentials`（generate/ai bootstrap）——T2 溯源不命中时保留字面凭据免手工回填，`generate_with_policy/ex` 链路透传） |
| E-4 | `{{var}}` 未解析门禁 | `resolver.rs:44,282-304`（字面量发送 + 事后警告） | `--strict-vars` 报错停止（默认维持警告不破坏既有语义） | 漏配变量不再打注定失败的请求 | ✅ 已落地（2026-11-13：`--strict-vars`——未解析 `{{var}}` 拒绝外发（executor 拦截 + 可读错误指名变量），默认语义不回退防假守护对照） |
| E-5 | 观测面补全 | `report/json.rs`（无 waterfall）；`reporter.rs:66-74` | JSON 报告补 timing_waterfall；终端渲染 `plugin_duration_us` | 离线 HTML 复渲染不退化；开发者看得见插件耗时 | ✅ 已落地（2026-11-13：JSON 报告 `timing` 分段（恒序列化保键集稳定，schema 精确字段测试演进）+ 终端 `plugin_duration_us` 渲染（"plugin +Xms"）） |
| E-6 | capture/teardown/depends-on 元数据入快照 | `model.rs:230-289` | 落元数据（capture 键名、teardown 标记、依赖边）；capture **值**仍不落 | 黑匣子含"捕获了什么、为何此序"证据 | ⏸ deferred（2026-11-13 裁决：captured_keys 需 4 层透传且**键名本身可能敏感**（api_key 类名外泄面）——诚实登记不做，待隐私评审后再议） |

## 5. P4 · 容器叙事与守护

| ID | 工作项 | 证据锚点 | 修复方向 | 验收标准 | 状态 |
|---|---|---|---|---|---|
| C-1 | rustls 切换 + musl job + 体积断言 | `Cargo.toml:26`（default-tls→openssl-sys）；`ci.yml` 无 musl job；`release.yml:27-33` 仅 gnu | reqwest 切 `rustls` feature（评估 tokio-tungstenite webpki-roots 兜底）；CI 加 musl target job + ≤20MB 断言 | Alpine/scratch 容器叙事成为工程事实 | 🔧 |
| C-2 | 守护补强三件 | `standalone_portable_test.rs:51,92-101`（仅清 PATH）；`pipeline.rs:477-489`（stderr 成功也入 warnings）；`registry.rs:581-585`（坏 manifest 缓存盲区） | env -i 级最小环境测试（含无 HOME）；stderr 成功过滤；`t` 缓存路径对坏 manifest 与 `plugin list` 行为一致 | 即拷即走承诺最后一道守护；warnings 语义纯净 | ◐ 部分落地（2026-11-13：env -i 最小环境测试 ✅ `test_minimal_env_i_no_home_no_panic`（env_clear+无 HOME 全链优雅退出）；插件 stderr 成功过滤 ✅（失败才入 warnings）；**缓存一致性子项回滚登记**——known_invalid 融合方案与既有"防抖动"裁决冲突且有盲区（orphan 补齐二进制后仍被 known 遮蔽），尊重既有裁决回滚，"t 路径坏 manifest 不可见"登记为已知取舍（用 plugin list 查看）） |

## 6. 方向性登记（生态期，不排期）

- **统一"会话资产"抽象**：token/cookie/变量由单一 session store 持有（带获取时间与失效检测），四轨道读写经它——R-1~R-5 落地前不做（避免过早抽象）。
- 生产重放防呆：`--target` host 与快照 URL host 同源警告（可随 B-2 顺带裁决）。
- 黄金样本变更检测实践规约（发布阶段金丝雀用法，依赖 B-2/B-5 先行）。

---

## 7. 依赖与施工顺序

1. **R-1 与 B-1 同批**：均收口 `app.rs/suite_setup` 装配路径，一次回归；
2. R-2 落在 R-1 之后：闭包执行解决"运行内免重跑"，var-state 解决"跨运行免重跑"，叠加才完整；
3. S-1 的默认脱敏评估依赖 E-3 先行验证模板链路无字面依赖；
4. C-1 独立赛道，随时可插。

## 8. 门禁要求（每批提交前）

- `cargo fmt --check` / `cargo clippy --workspace --all-targets -- -D warnings` 零漂移；
- `cargo test --workspace` 全绿；
- `tests/verify_features.sh` exit 0（涉及重放/变量/会话项需增补对应冒烟步骤）；
- `examples/run_all.sh` ALL PASS。

---

## 审查修复登记（2026-11-13，code review P0/P1 全项）

外部 code review 发现 2×P0 + 10×P1，全部修复（`fix(review)` 提交）：
- **P0-1** S-1 首建截断竞态：`File::create`→`OpenOptions::create_new + mode(0o600)`（mode 原子生效，无截断/无 0644 窗口；并发首建败方走 AlreadyExists 收紧分支）
- **P0-2** E-3 keep_credentials 产物 0644 → `write_private_file_atomic` 0600 + 真实警告（generator/bootstrap 两处）
- **P1-1** S-2 sha 三态语义 → model.rs SSOT 注释分态化 + 02 附录裁决登记 + 截断×掩码组合测试
- **P1-2** S-3 assertion 四字段（expression/actual/expected/message）补值掩码 + 表面测试
- **P1-3** E-3 query 敏感键豁免透传（`variabilize_url/query_pairs` 链路，与 header 语义一致）+ P1-4 豁免/对照双测试
- **P1-5** E-5 `from_json_file_reports` 接线 JsonTiming（diagnose → timing 分段 → 总时长三级回退，离线复渲染兑现）
- **P1-6** C-2 stderr 推送移到 failure 解析后（"exit 0+非法输出"形态诊断不再丢失）
- **P1-7/P1-8** C-1 声明修正（ring→aws-lc-rs，musl 需 cmake）+ CI 装 cmake + CHANGELOG/E-1 档案登记 scratch HTTPS 需自带 CA bundle 边界
- **P1-9** E-2 宏表与 resolver SSOT 对齐（删 `$timestamp_s`，补 `$uuid/$timestamp/$timestamp_iso`）
- **P1-10** registry.rs 两段描述已回滚行为的假文档 → 改写为已知取舍说明
- **P2 顺手**：S-3 值替换长度降序、`ensure_dir` doc 归位、commands schema 补 `requires_env`

## 审查修复登记（第二批，2026-11-13 P2 项 + R-4）

- **R-4 落地**（见上表翻转）：Web 会话延续补齐——免重填三轨道在 Web 轨道闭环。
- **P2-4**：S-3 e2e 断言收紧（精确 `"masking": "secrets"` 解析断言）。
- **P2-5**：E-4 strict_vars 对 base_url/baseUrl 特判（交由 BaseUrlNotConfigured 专用处方分支，错误形态一致）+ 错误消息注明字面量 `{{}}` 拦截语义；交互入口（TUI/Web）默认 strict_vars=false——选项面透传登记为备忘。
- **P2-6**：`--strict-vars` help 注明"body/URL 中合法 `{{ }}` 字面量也会被拦截"。
- **P2-7**：终端插件耗时渲染改 `div_ceil`（<1ms 显示 +1ms，不再 +0ms）。
- **P2-8**：`test_json_timing_subkeys_and_skipped_form_key_parity`——JsonTiming 子键精确锁定 + skipped 形态键集合一致性。
- **P2-9**：manifest description ≤80 校验落地（registry 校验与 schema manifestSchema 对齐，03 §6.4）+ 拒绝测试（含路径修正：`.rupost/plugins`）。
- **review 遗留说明**：P1-7 后端为 aws-lc-rs（非 ring）、musl 需 cmake（CI 已装）——CHANGELOG/E-1 已修正声明；P2-2 Windows history 权限靠 ACL 兜底（CHANGELOG 已登记）；P2-3 值替换长度降序已落。
