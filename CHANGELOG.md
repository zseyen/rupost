# Changelog

All notable changes to RuPost are documented in this file.

Format based on [Keep a Changelog](https://keepachangelog.com/); versioning follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed

- **MSRV raised to Rust 1.89** — the codebase now uses std file-locking APIs (`File::lock` / `File::lock_shared` / `File::unlock`) stabilized in 1.89; the `fs2` dependency has been removed.
- `@timeout` is now enforced per-request on non-streaming HTTP requests (previously parsed but never applied; the client-wide 30s cap still applies as fallback). Requests declaring `@sse`, `@websocket`, or `Accept: text/event-stream` are exempt — reqwest's per-request timeout is a total-duration cap that would otherwise kill long-lived streams mid-flight.
- Failed `@capture` directives and unresolved `{{variable}}` placeholders are now surfaced as visible per-request warnings instead of being silently dropped / sent as literals.
- Cookie files are now written with `0600` permissions (they hold session credentials; previously `0644` via umask) and saved with a lock-safe sequence: truncation happens under the file lock and buffered data is flushed before unlock, eliminating a window where a concurrent reader could see an empty or half-written file.
- Cookie persistence granularity moved from per-response to per-test-file completion in serial batch runs — crash-loss window widens from one request to one file. `after_response` no longer writes; parallel mode has never persisted `--cookie-file` state (workers use ephemeral stores). Every response no longer triggers a synchronous full-file rewrite.
- Inline (curl/httpie-style) requests now resolve `--cookie-file` through `--env` (`cookies_dev.json`), matching the test-suite path; a new global `--env` flag was added.

### Fixed

- Non-standard HTTP status codes (600–999) are accepted and reported as failed test results instead of panicking.
- Assertions whose expected value is a lone quote character (e.g. `@assert body contains "<"`) no longer panic.
- Invalid HTTP header names (e.g. containing spaces) produce a failed test result instead of panicking.
- Assertion operators are now located at the leftmost position with quote awareness — `@assert body.path contains "<b>x</b>"` is parsed as written instead of being mis-split on `<`; WS `EXPECT` conditions additionally support `>=`, `<=`, `>`, `<` (previously silently never matched).
- Relative URLs whose query string contains `://` (e.g. OAuth callback params) are joined against `base_url` again instead of being treated as absolute.
- Explicitly passed test files outside the current working directory run instead of being rejected as "safety violation"; uppercase `.MD` files route to the Markdown parser.
- Request timeouts produce a clear "请求超时 (request timed out)" error instead of a generic send failure.
- The `BaseUrlNotConfigured` suggestion now points to `--var` instead of the non-existent `-v` variable flag.

### Planned (hardening campaign, see `doc/plans/2026-10-18-quality-hardening-execution-plan.md`)

- **Breaking**: SSE events with no `data:` line will no longer be dispatched (WHATWG conformance); `event:` field resets, `id` (lastEventId) persists.
