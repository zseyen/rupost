# Changelog

All notable changes to RuPost are documented in this file.

Format based on [Keep a Changelog](https://keepachangelog.com/); versioning follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- TUI Response 面板现在渲染 `TestResult.warnings`（`Warnings: N` 摘要 + `⚠` 逐条明细）：`@capture` 失败、未定义变量、畸形 JSON 降级等非致命告警此前在 TUI 中被静默丢弃（`RequestFinished` 事件未携带），存在"断言全过但实际带伤"的假通过盲区（D1 遗留项关闭）。
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
