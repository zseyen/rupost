# Changelog

All notable changes to RuPost are documented in this file.

Format based on [Keep a Changelog](https://keepachangelog.com/); versioning follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

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
