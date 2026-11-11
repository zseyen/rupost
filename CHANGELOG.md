# Changelog

All notable changes to RuPost are documented in this file.

Format based on [Keep a Changelog](https://keepachangelog.com/); versioning follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- **`--var` 非法格式告警 + toml `${VAR}` 未设置告警 + 变量装配 DRY 收口（走查缺口 #6 家族收尾，walkthrough-gaps 合流 Step 5A）**：①`--var no_equals`（缺 `=` / 空键）此前经 `filter_map(parse_cli_var)` 静默丢弃——用户显式传参落空必须告警（与 `--env-file` 同语义；stderr 出站，`--report json` stdout 机器可读纯净不破坏，E2E 锁定退出码 0）；②toml 环境值引用的未设置 `${VAR}` 此前静默按字面量外发（`{{var}}` 残留检查覆盖不到 `${}` 形态）——现指名告警 + 同变量多键引用去重（`resolve_env_vars_with_warnings`；热路径 `resolve_env_vars` 签名不变，小写 `${x}` 不展开不告警既有语义锁定，防 `${path}` 类误展开）；③流式日志截断 marker 补 `flush()`——防下一次 metadata 采样滞后重复写。**DRY 收口（AGENTS §9）**：5 处重复的 5 层变量装配模板（runner/suite_setup、exporter、tui、web 单块/整套）收敛为 `ConfigLoader::parse_cli_vars_with_warnings` / `load_context` / `load_context_with_var_specs` 三入口（I/O 加载与告警渲染分离，渲染权归各适配层：CLI stderr / TUI 状态栏 / Web tracing）。

- **配置/env 静默失败告警化（走查缺口 #6，DRY 封装）**：四处"用户已表达意图却静默吞错"的加载点全部告警化——①`--env <name>` 两层同时落空才告警——toml 存在其他环境段但不含目标名 **且** `.env.<name>` 也未找到（审查精度修正：toml 无任何环境段时不告警，`--env` 可能仅用于 cookie 隔离/env 文件模式，防告警疲劳）；②显式 `--env-file` 缺失/目录/解析失败（原契约测试锁定的静默行为反转，反向守护保持默认 `.env` 缺失静默——防告警过宽）；③`rupost.toml` 存在但损坏（原 `.ok()` 把解析失败与"没有配置"混同、环境变量层被静默清空且继续向父目录回退——现携带根因停止搜索）；④SSE/WS 流式日志写入失败（原 `let _ =` 吞掉每事件落盘错误，现 once 闸门首次失败告警进 warnings 随结果出站；审查补漏：WS 脚本模式 Send/EXPECT 两处同款静默点——守护测试先红后绿暴露）。封装：`env_file::load_env_file(path, explicit) → EnvFileOutcome` 为三个 env 加载点唯一决策点（决策表测试锁定）；`find_and_load`/`build_context` 返回 `(值, warnings)`，各入口自行渲染（CLI stderr / TUI 状态栏 / Web tracing）。语义裁决：存在即意图——文件损坏恒告警，缺失是否告警取决于显式与否；`.env.<env>` 为可选覆盖文件（缺失静默，防告警疲劳）。
- `--report junit` 报告新增 `@timestamp`（RFC3339 聚合时间）与 `@hostname`（`RUPOST_RUN_HOST` > `HOSTNAME` env > 空串兜底，零新依赖）锚点（走查缺口 #3）——CI 平台按此归档与跨构建比对，此前仅相对 `time`。
- `rupost --no-history`（走查缺口 #1）：**运行级历史落盘停用开关**——CI/共享环境防凭据明文落盘（此前每次 `rupost t` 都把完整 headers/body 写入 `.rupost/history.jsonl`，与报告出口的强制脱敏在同一进程内不一致）；全局旗标（子命令前后双序 + 内联模式，位置契约测试锁定）；派发层在任何请求执行前关停记录器（进程级不可逆，与测试进程静音同一原语）；停用提示走 stderr，`--report json` stdout 机器可读纯净不破坏。
- **连接层诊断瀑布补齐（走查缺口 #5，DX-Debug D2 拆分）**：传输层失败（DNS/TCP/TLS）此前产出零诊断——恰在最需要诊断的场景断裂。`DiagnosticsProber::connect_tcp_staged` 分阶段探测（失败不早退，保留已测 DNS 阶段与已解析 IP，携带阶段分类失败信息；严格版 `connect_tcp_with_timeout` 委托之、错误文案不变）；`DiagnosticsReport` 新增 `connection_error` 字段（始终序列化 None→null，成功/失败形态键集合一致，schema 锁定 13→14 键）；`diagnose_url_lenient` 任一阶段失败均产出部分诊断报告（DNS/TCP/TLS/HTTP 全阶段降级——审查扩展把 TLS 阶段一并收口；严格版 `diagnose_url` 与 diagnose 命令的 Err 契约不变）；`http_runner` 传输错误路径换用宽容诊断——`--debug-on-failure` 在连接拒绝/证书损坏时也能给出 `Connection: TCP connection failed: ...` / `TLS handshake failed: ...` 分类瀑布（DNS 成功时 resolved_ips 非空可区分阶段）。
- **HTML 报告失败片段同源脱敏（走查缺口 #2，审查扩展至全字段）**：在线直渲路径此前整体弱于 JSON 出口——body_snippet 仅 512 字符截断（`redact_and_truncate_body` 对齐），url/error/warnings/断言 expected-actual-message 裸奔（审查发现后补齐 `redact_url`/`redact_free_text`/`redact_assertion_value` 同一原语）——CI 归档的 `rupost-report.html` 可能携带失败响应中的 token/api_key 回显与 query 凭据。
- **HTTP 请求体原样保真与零分配语法嗅探 (Raw Body Verbatim & Zero-Alloc Sniffing)**：
  - `Request` 模型引入 `raw_body: Option<String>` 与 `with_verbatim_body`；
  - `add_body` 采用 `serde_json::from_str::<serde::de::IgnoredAny>` 进行零堆分配合法性嗅探；
  - 发包底层原样保真发送原始文本，彻底根除键字典序洗牌、空白折叠与浮点精度变形，确保 API 加签校验 100% 保真。

- **快照全量黑匣子录制与 Schema v2 演进 (Full Snapshot Recording & Schema v2)**：
  - 引入强类型 `ExecutionErrorDetail`，`SnapshotEntry` 演进为 `response: Option<ResponseSnapshot>` 与 `error: Option<ExecutionErrorDetail>`；
  - 解除快照落盘时的 `r.response.is_some()` 过滤守卫，超时、断网与连接被拒等网络异常 100% 录入黑匣子快照；
  - `SnapshotSuite` 支持 Schema `version: 2`，借助 `#[serde(default)]` 保证与 Schema v1 双向平滑兼容；
  - `mask_snapshot_entry` 联动 `redact_free_text`，对 `error.message` 中潜伏的 URL Query / Basic Auth 凭据进行安全脱敏。

- **全链路测试基建补强与守护套件 (Full Testing Baseline & Guard Suites)**：
  - **格式化器直测**：`ResponseFormatter` 扩充 `with_color`、`with_body`、`with_headers`、`with_timing` Fluent Builder 接口，补齐全量表驱动单测；
  - **TUI 关键动作与守卫直测**：新增 `tests/tui_key_actions_test.rs`，覆盖 `start_ws_session` 互斥防线与 `replay_latest_snapshot` 错误流，并通过 `TestBackend` 离屏验证；
  - **Web 前端原生测试基建**：微调 `app.js` 环境安全守卫，基于 Node.js 原生 `node --test` 与 Micro-DOM 打桩建立 `tests/web/app_test.mjs`（零 npm 依赖），`Makefile` 接入 `make test-web`。

- **凭据脱敏扩展性与合规治理 (Redaction Extensibility & Compliance Guard)**：
  - 核心策略控制中心 `RedactionPolicy`：表驱动三层过滤（`EXACT_KEYS` 硬键 $\to$ `ignore_keys` 豁免 $\to$ `SUBSTRINGS` 模糊匹配），二分查找与静态切片常量化；
  - R3 硬键安全护栏：`Authorization`、`Cookie` 等 8 项 RFC 认证硬键绝对不可豁免，配置尝试豁免时启动即阻断防呆并输出黄色告警；
  - 配置文件级联驱动：支持在 `rupost.toml` 中配置 `[export.redaction]` 的 `extra_keys` 与 `ignore_keys`；
  - 请求级双向微调：支持在 `.http` 文件中通过 `# @sensitive <keys>`（追加脱敏）与 `# @no-redact <keys>`（局部豁免），支持逗号与空白分隔多值解析及大小写规范化；
  - 导出审计清单 Footer：`rupost export` 导出脱敏命令时，在 stderr 输出格式化脱敏清单摘要（凭据键名、认证类型、字符数与来源维度），stdout 纯净命令管道不被破坏，支持 `--quiet` 静音；
  - 模板变量清洗：`HttpGenerator` 自动将 Header/Query 键名规范化清洗为紧凑合法的变量名（去除首尾及连续下划线，空值回退为 `secret`）。

- `@teardown` 确定性生命周期（M3）：请求块标记为清理步骤——常规阶段占位跳过（原序号保留），批处理 Finally 阶段按已执行文件逆拓扑序执行；变量防呆（渲染副本检残留，未解析 → skipped + 警告，杜绝脏请求外发）；`TestResult.is_teardown` 标记 + 快照导出默认排除（防 replay 重放 DELETE）；失败继续全部清理、最终聚合失败；parallel + fail_fast 中断路径修复（JoinError::is_cancelled 容忍，变量合并与 Finally 不再被跳过）。
- `rupost mock` 智能 CORS（M3，默认开启）：预检 OPTIONS 秒回 204 + 动态回显 Origin + Credentials: true + Allow-Headers 回显预检请求头（非 `*`——credentialed 请求合规）；`--no-cors` 停用；无 Origin 时 `*` 形态；`Vary: Origin`。
- `rupost t --report junit / --report-file <PATH>.xml`（M1/T8）：**工业级 JUnit XML 测试报告**——100% 符合 Jenkins/GitLab/GitHub Actions 消费规格；基于 build_json_reports 统一脱敏结果生成（SSOT 复用，敏感值不外泄）；终端 ANSI 控制符构造期彻底剥离（防 XML 解析异常）；`<failure>`（断言失败）与 `<error>`（请求异常）节点区分（Jenkins 红绿语义）；`--report junit` 无文件默认落盘 rupost-junit.xml；junit_schema_test E2E + verify_features 步骤 14。
- `rupost web --dev`：**开发者跨域模式**——挂载宽松 CORS 供独立前端（Vite/Webpack）跨域联调；安全姿态三层：非回环 `--host` fail-fast 拒绝（bind 之前）、启动横幅明示浏览器侧威胁（任意网页可跨域读写本地文档）、Host 守卫与回环绑定叠加；非 dev 模式零 CORS 头（默认拒）。
- **快照脱敏（opt-in）**：`rupost t --save-snapshot` 与 `rupost history export` 新增 `--mask-secrets`——headers 走凭据掩码（Bearer 前缀保留）、body 走敏感键 `[REDACTED]` + PII 三正则（手机号/银行卡/邮箱，无截断）；schema 不变，掩码产物仍可 `replay` 加载；默认关（重放 body 比对保真度优先）。
- **Breaking 候选**：`replay` 升级为状态码+body（默认 14 键动态排除后）双比对——body 失败计 failed 并 exit 1（逃生舱 `--status-only`；`--ignore-fields` 追加排除键）；同批解冻 AI 接线：`replay --explain` 失败条目就地归因（规则版基线 + AI 增强，无 Key 降级）。生成语义线：T1 动态值反向模板化（`{{""}}`/`{{"$timestamp"}}`/`{{"$timestamp_iso"}}`/`{{"$timestamp_ms"}}`，敏感键凭据模板化优先）；T2 链式值溯源（上游 `` + 下游 `Bearer {{"{{token}}"}}`，Cookie 链删头靠罐，生成文件直接重跑跑通）。
- `rupost replay` 失败定位到条（DX-Debug R6）：重放结果升级为结构化 `ReplayEntryResult`（1-based 序号/期望与实际状态码/耗时/传输层错误首行，serde 双向），终端汇总后输出 `Failed entries` 失败清单块（0 失败零噪音）；任一条目失败进程 exit 1（CI 门禁语义，与 `test` 同款）。聚合 Total/Passed/Failed 语义不变。AI 预留桥 `context_from_replay_entry`（URL/错误文本防御性脱敏）使 replay 失败可直接流入既有 `--explain`/`ai triage` 归因管线（v1 建桥未接 CLI，fast-follow 解冻）。
- `rupost history export --last <N> -o <FILE>`（T5，随 R6 同批）：**历史 → 可重放快照**桩转实——最近 N 条历史条目经既有快照转换导出（`body=None` 旧条目占位导出，条目可追溯）；空历史不写文件 exit 0；产物可直接 `rupost replay`。
< Breaking 候选：`replay` 升级为状态码+body（排除动态键后）双比对，body 失败计 failed 并 exit 1（逃生舱 `--status-only`）+ `--ignore-fields` 追加排除；`replay --explain` 失败条目就地归因（规则版基线 + AI 增强，B1 降级）。新增 `history export`→重放供给链与生成语义两线：T1 动态值反向模板化（{{$uuid}}/{{$timestamp}}/{{$timestamp_iso}}/{{$timestamp_ms}}，敏感键跳过）；T2 链式值溯源（上游 `@capture` + 下游 `Bearer {{token}}`，Cookie 链删头靠罐，生成文件直接重跑）。
- `rupost test --on-failure <CMD>`（DX-Debug R7）：**失败钩子与 trace 锚点协议**——每个失败请求触发一次用户 shell 命令（`sh -c`），注入 `RUPOST_FAILED_URL`（脱敏）/`RUPOST_FAILED_STATUS`/`RUPOST_FAILED_TRACE_ID`（按 `Traceparent > X-Request-Id > X-Trace-Id` 优先级提取）锚点环境变量；钩子 stderr/stdout 一律透传至 rupost 的 stderr（保 `--report json` stdout 机器可读纯净）；10s kill-on-drop 超时（如实上报）；钩子超时/非零退出/未检出 trace 头（空串 + warnings 可见）均不改写测试退出码。rupost 只给锚点，服务端日志聚合委托用户管道（stern/k9s）。`FailureContext` 同步携带 `trace_id` 供 `--explain` AI 归因引用。
- TUI 接入剩余 CLI 功能：`V` 重放最新快照（Results tab 呈现对比报告）、`M` 开关当前文件 mock 服务器（自动空闲端口 + 状态栏指示）、`I` AI 断言翻译（自然语言 → `# @assert` 行追加编辑器）、`--cookie-file` 磁盘持久化（启动加载 + 触发末尾写回）；mock/server.rs 拆分 build/serve 以供编程接入。
- TUI 接入 CLI 已实现能力（执行模型对齐 + 调试增强 + 快照/生成）：①整文件多请求顺序执行（变量级联、@skip），Results 详情 tab 逐条显示；②单请求选择器（`#` 模态，`#3`/`@name`/裸序号，复用 runner selector 纯函数）；③cookie 会话延续（触发间经 cookie_bridge 保持，不再每次重置）；④响应面板四段耗时行（DNS/TCP/TTFB/Transfer）；⑤失败自动诊断（debug-on-failure 语义）+ Diagnose tab 原生渲染；⑥`X`/`Shift+X` 导出 curl（[REDACTED]/原样）；⑦`Ctrl+S` 快照保存（默认脱敏）；⑧History `G` 一键生成语义化命名 `.http`。executor 新增 `execute_all_streaming`（逐请求回调），`execute_all` 委托之。
- TUI 流式调试深化：①启动加载变量上下文（rupost.toml 环境 + .env + 系统变量，`--env/--var/--env-file` 透传，与 CLI 同源）——`Authorization: Bearer {{token}}` 等授权变量在 TUI 真正解析；②`A` 快速授权（输入 header 值即注入，显式声明优先）；③Response 面板 `/` 消息搜索（HTTP/SSE/WS 三视图实时高亮 + n/N 环绕跳转，顺带修正滚动总行数与视觉行不一致）；④WS 交互会话（`W` 对 `@websocket` 请求建立常驻会话、`S` 发送消息实时回显、`D` 关闭，断线自动重连沿用 session 逻辑，与剧本执行互斥）。
- TUI Response 面板现在渲染 `TestResult.warnings`（`Warnings: N` 摘要 + `⚠` 逐条明细，含 SSE/WS 流式完成态视图首行摘要）：`@capture` 失败、未定义变量、畸形 JSON 降级等非致命告警此前在 TUI 中被静默丢弃（`RequestFinished` 事件未携带），存在"断言全过但实际带伤"的假通过盲区（D1 遗留项关闭）。顺带修复 HTTP 缓存渲染管线扁平化导致的样式丢失——未超宽行保留 Span 主题取色。
- `rupost ai bootstrap --from history --last N -o FILE [--filter] [--no-ai] [--templatize-host]`（F8）：**调试历史 → 语义增强回归文件**——五阶段管线（采集 → `HttpGenerator` 机械基线 → `TestSynthesizer` AI 语义增强〔分块批量 + 连续 2 错熔断〕→ 引擎裁决〔断言 parser lint + capture 语法校验 + 每块 ≤8 条〕→ 防覆盖落盘）；AI 缺失/故障降级为机械基线 + 黄色警告 + exit 0（B1）；素材发送前脱敏 4KB 截断。
- `rupost ai resolve <FILE> [--dry-run]`（F7）：文件内 `# @ai-assert <自然语言>` 指令**原地展开**为标准 `@assert` 行（B2：AI 全局失败文件原样 + exit 1；部分失败保留原指令 + 行内警告；dry-run 预览；Top-20 配额 + 熔断；展开断言经 parser 本地预检，非法丢弃）。
- `rupost import curl "<curl>" [-o FILE] [--templatize-host]`：**curl → 可执行 `.http` 请求块**（通用导入器，与 `export` 互逆）。POSIX 子集分词（单/双引号/反斜杠；`$'..'`、`-F`、`--data-urlencode`、`-u` 显式报错）；发射复用 `HttpGenerator`（Header/URL query 凭据模板化为 `{{变量}}`；请求体暂不模板化，含敏感键时落盘前黄色警告、`@name` 语义命名、JSON 美化、status 断言）；`-o/--output` 与 `--templatize-host` 可置于任意位置（trailing_var_arg 拦截；CURL 序列内的 `-o` 覆盖 curl 自身文件输出语义——devtools 复制场景不含 `-o`）；输出文件防覆盖。
- **B4 配置链**：AI 端点解析升级为 `环境变量 > ~/.rupost/ai.toml > 本地探测` 三级——文件含 `api_key` 字段即拒绝（密钥永不入盘）；本地回环端点允许占位 Key（Ollama 不校验鉴权）；零配置时自动探测本地 Ollama（`GET /v1/models`，2s 超时，`RUPOST_AI_PROBE_URL` 为测试钩子）并打印一次性提示；全部失败时输出三选一可执行指引。
- `rupost ai assert --last <N> / --response <FILE>`：**上下文感知断言**——取最近 N 条历史（或指定响应文件）经脱敏截断（4KB/条）后作为实际响应上下文，AI 依据真实 JSON 结构生成精确到实际路径的断言；`--no-mask` 显式关闭上下文脱敏（输出一次性醒目风险提示）。
- `rupost test --explain`（别名 `--ai-explain`，支持 `RUPOST_AI_EXPLAIN=1`）：断言失败**就地归因**（E2）——恒输出确定性规则版 `[Possible Causes]` 基线，配置 AI 后按**不同错误签名**追加 `[AI Explanation]` 根因诊断（Top-3 配额 + 连续 2 错熔断）；退出码严格由测试本身决定。
- `rupost ai triage <report.json> [--json|--format]` 与 `rupost test --triage`：CI 失败**根因聚类分诊**（E5）——确定性签名分桶（数字/UUID 折叠 `:id`，上限 20 桶）→ AI 语义归并 → **分桶 ID 守恒回填**（计数 100% 守恒、零幻觉 ID）；无 Key 时降级确定性分组并输出黄色警告，exit 0。
- `rupost test --report-file <FILE>`：JSON 报告落盘（不干扰终端输出）；报告升级为强类型 `JsonFileReport` 契约，失败项附带脱敏（复用 exporter 脱敏器 + JSON 敏感键掩码 + 512B UTF-8 安全截断）的响应体片段与断言 expected/actual。
- `rupost ai assert "<自然语言>"`：AI 创作辅助命令，将自然语言期望翻译为标准 `@assert` 断言行（支持任意 OpenAI 兼容后端与本地 Ollama；密钥仅从 `RUPOST_AI_API_KEY`/`OPENAI_API_KEY` 环境变量读取，不落盘不入日志）。`--raw` 输出纯断言行供脚本追加。休眠的 `rupost-ai` crate 正式接线激活；通过 `rupost mock` 充当 OpenAI 兼容后端实现零 Key 离线 dogfooding 闭环（`tests/ai_assert_test.rs` + `tests/verify_features.sh` 步骤 7）。

### Changed

- **报告文件输出上移表现层 + 格式判定显式优先**（分支收尾批次）：`run_test_suite` 返回 `TestSuiteOutcome { has_failure, batch_results }`（**破坏性 API 变更**：不再返回 bool，UseCase 不再隐式写报告文件）；渲染+落盘收敛为 `rupost::report::write_report_files` 门面。格式判定改为**显式 `--report` 永远优先**（忽略大小写，统一此前 stdout 精确匹配与文件判定忽略大小写的口径不一致）：`--report json --report-file x.html` 现在写 JSON（旧行为：扩展名压过显式格式静默写 HTML）；默认 terminal 按扩展名推断不变。报告文件已存在时 stderr 提示 `[!] 报告文件已存在，将覆盖`（允许覆盖不阻断，CI 友好；CLI 安全评估 P1-1 闭环）。空扫描提示 `No test files found.` 改 stderr（`--report json` stdout 纯净）。
- **双角色 mock 语义登记**（既有行为，随批次正式入档）：`rupost mock` / `build_mock_matcher` 按**含 `@mock-when`/`@mock-default` 变体的契约块**编译路由，与 `@test` 标记无关——纯契约块（无 @test 有变体）会被服务，纯 `@test` 无变体块不再注册 mock 路由；`MockCompiler` 不可达兜底分支删除。
- `rupost g` (history → .http generation) now **templates credentials as
  variables** instead of writing them verbatim: `Authorization: Bearer eyJ...`
  becomes `Authorization: Bearer {{authorization}}` (scheme prefix preserved,
  variable carries only the credential body), sensitive URL query values become
  `api_key={{api_key}}`, and a header comment lists all referenced variables
  (provide values via `--var` / `.env` / `rupost.toml`). Previously generated
  files contained plaintext credentials — a security fix first shipped as
  `[REDACTED]` placeholders, now upgraded to runnable variable references.
  `rupost export` keeps `[REDACTED]` (its output targets humans/pasting).
- `rupost g` now emits **semantic names**: `### GET /users` and
  `# @name get_users` (derived from method + path, ordinal suffix on
  collision) instead of opaque `### Request <id8>` / `req_<timestamp>_<n>`.
- `rupost g --templatize-host` rewrites the absolute origin of each request to
  `{{base_url}}/path` so one generated file runs across environments via the
  standard variable cascade (naming follows the `rupost.toml` template
  convention). Default behavior is unchanged (absolute URLs).

### Fixed

- scenario/html_report 集成测试基建去 flake：7 处固定端口（19188-19194/19198）+ `let _ =` 吞 bind 错误 + sleep 启动竞态，统一改为 `bind("127.0.0.1:0")` + `serve_matcher`（已绑定 listener 直传，无 re-bind TOCTOU），端口占用从静默误导失败变为确定性 panic；同批清偿 `mock_integration_test.rs` 存量 6 处同款（19090-19095，含单测试三实例并存场景）——in-process 测试面"固定端口 + 吞错 + sleep"坏模式清零（`ai_assert_test` 子进程形态的固定端口 + `wait_for_port` 轮询豁免；实测占用端口时旧代码报误导性 `Failed to connect`，新代码 bind 处立即 `AddrInUse` 精准 panic）。
- `rupost t` 失败判定语义新增子进程 e2e 锁定（失败 exit 1 / 全绿 exit 0）。
- 空扫描（目录内无 .http/.md）恢复旧语义：不产生任何报告文件（迁移期曾短暂改为写空报告，经首轮代码审查捕获）；`--report json` 的 stdout 判定与 `--report-file` 扩展名推断补齐大小写不敏感（`--report JSON`、`OUT.HTML` 不再静默走错分支）。
- **Security**: `rupost g` no longer writes `Authorization` / `Cookie` /
  `X-Api-Key` header values or sensitive URL query values (e.g. `?api_key=`)
  verbatim into generated `.http` files. Redaction semantics are shared with
  `rupost export` (`exporter/redact.rs`).

- TUI: file creation I/O is routed through a `FileRepository` port (atomic
  `create_new`, Fs/InMemory implementations) — the ViewModel no longer touches the
  filesystem directly on the write path.
- TUI: free-form input (CLI arguments and TUI quick input) now shares one URL
  normalization predicate (`http::url::normalize_input_url`); `:3000/x` localhost
  shorthand and explicit ports in derived filenames work in both.
- TUI: quick input uses `ratatui-textarea` behind an exclusive `InputMode` state
  (correct multibyte/IME editing, real terminal cursor instead of a fake block glyph).
- TUI: handlers split into component modules behind a pure navigation layer
  (`nav.rs`); state split into `state/{tree,layout,stream,history,wrap,retention}`.

- **MSRV raised to Rust 1.89** — the codebase now uses std file-locking APIs (`File::lock` / `File::lock_shared` / `File::unlock`) stabilized in 1.89; the `fs2` dependency has been removed.
- `@timeout` is now enforced per-request on non-streaming HTTP requests (previously parsed but never applied; the client-wide 30s cap still applies as fallback). Requests declaring `@sse`, `@websocket`, or `Accept: text/event-stream` are exempt — reqwest's per-request timeout is a total-duration cap that would otherwise kill long-lived streams mid-flight.
- Failed `@capture` directives and unresolved `{{variable}}` placeholders are now surfaced as visible per-request warnings instead of being silently dropped / sent as literals.
- Cookie files are now written with `0600` permissions (they hold session credentials; previously `0644` via umask) and saved with a lock-safe sequence: truncation happens under the file lock and buffered data is flushed before unlock, eliminating a window where a concurrent reader could see an empty or half-written file.
- Cookie persistence granularity moved from per-response to per-test-file completion in serial batch runs — crash-loss window widens from one request to one file. `after_response` no longer writes; parallel mode has never persisted `--cookie-file` state (workers use ephemeral stores). Every response no longer triggers a synchronous full-file rewrite.
- Inline (curl/httpie-style) requests now resolve `--cookie-file` through `--env` (`cookies_dev.json`), matching the test-suite path; a new global `--env` flag was added.
- **Breaking**: SSE events with no `data:` line are no longer dispatched (WHATWG conformance). Previously `event: ping` heartbeats produced empty events and counted toward `sse_max_events`. On such frames the event type resets; the id (lastEventId) persists and travels with the next dispatched event.
- `Content-Type: application/json` bodies that are not valid JSON are now sent as-is with a visible warning instead of being rejected locally — a test tool must be able to deliver malformed payloads to verify a server's 400/422 handling. The user-declared Content-Type is preserved.
- HTTP snapshots now record `@timeout` and replay honors it (previously replays silently fell back to the client-wide 30s cap); the `.http` generator re-emits the directive. Old snapshots without the field replay as before.
- Assertions are now evaluated against the leftmost operator with quote awareness, so `contains "<b>x</b>"` style assertions parse exactly as written (shared implementation with WS `EXPECT`).
- `--env` names containing path separators or `..` are rejected — they would write cookie files outside the expected directory.

### Fixed

- **TUI quick-input file creation no longer escapes the workspace**: paths with `../`,
  absolute paths or backslashes are rejected with a visible status-bar error (previously
  written outside the workspace and failures were silently swallowed via `let _ =`).
- Quick-input write failures (permissions, disk full) surface as a red status-bar message
  instead of being dropped; already-existing files keep the not-overwrite behavior.
- URL-derived test filenames are bounded (≤80 chars + short hash) instead of overflowing
  filesystem name limits; plain filename input gets the same bound.

- Non-standard HTTP status codes (600–999) are accepted and reported as failed test results instead of panicking.
- Assertions whose expected value is a lone quote character (e.g. `@assert body contains "<"`) no longer panic.
- Invalid HTTP header names (e.g. containing spaces) produce a failed test result instead of panicking.
- Relative URLs whose query string contains `://` (e.g. OAuth callback params) are joined against `base_url` again instead of being treated as absolute.
- Explicitly passed test files outside the current working directory run instead of being rejected as "safety violation"; uppercase `.MD` files route to the Markdown parser.
- Request timeouts produce a clear "请求超时 (request timed out)" error instead of a generic send failure.
- SSE streams that error mid-flight now flush and process the pending tail event (e.g. a `data: tail` line followed by an abrupt disconnect) instead of silently dropping it.
- The SSE parser's line buffer and per-event data accumulation are capped at 1 MB each (warn + truncate, reset at event boundaries) — hostile streams could previously grow both without bound.
- Markdown code fences with info strings like ```` ```http title="demo" ```` are recognized (language matched by first token).
- The JSON report (`--report json`) now includes the per-request `warnings` field — CI consumers previously lost all capture/unresolved-variable warnings.
- The `BaseUrlNotConfigured` suggestion now points to `--var` instead of the non-existent `-v` variable flag.

### Removed

- Dead code: the never-invoked `run_security_lint` utility and an orphaned `metadata_types.rs` module.
