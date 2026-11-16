# Mock 与 Web 工作台对齐实施计划（Flash 级可执行规范）

> **目标**：打通 US-1 ~ US-5 在 Web 工作台与 Mock 核心引擎之间的断点，消除仿真器双脑裂、补齐原地热重载与实时流量观测能力。  
> **前置基础**：`TrieNode::upsert`、`EvalContext`、`MockTrafficEvent`、`serve_matcher_with_events` 与 `MockCompiler::try_compile` 已在主干就绪，本计划直接复用，**禁止重复造轮子**。

---

## 0. 执行铁律（接手模型必读）

1. **Clean Imports**：禁止在函数体、结构体初始化或测试代码内书写 3 段及以上全限定路径（如 `std::sync::Arc::new`、`crate::mock::...`），统一在文件顶部或 `mod tests` 顶部 `use` 引入（仅 `std::io::Result` 消歧除外）。
2. **向后兼容与全量编译检查**：
   - 所有新增到现有 Request/Response DTO（`src/web/types.rs`）的字段，**必须标注 `#[serde(default)]`**，确保旧客户端或既有测试 JSON 反序列化不报错。
   - **每次修改 DTO 后必须跑 `cargo check --bins --tests`**，确保 `tests/` 下的结构体字面量同步更新（不可只跑 `cargo test --lib`）。
3. **Axum 0.8 语法警告**：本项目使用 `axum 0.8`，动态路径段**必须**写为 `"/history/{id}"`；严禁使用旧版 `"/history/:id"`（否则会在 `Router::route` 时直接运行时 panic）。
4. **只读守卫**：任何修改状态或启停服务端的 Web Handler，必须在首行检查 `if state.readonly { return (StatusCode::FORBIDDEN, ...); }`。
5. **TDD 三步法**：每个 Task 必须先写 Red 测试并确认失败，再写最小实现变绿，最后跑全量相关测试与 `cargo check --bins --tests`（零编译告警）。
6. **原子化提交**：每完成一个 Task，执行一次 `jj commit -m "..."`。

---

## Task 1 (P0 · US-1)：修复 `MockSimulate` 双脑裂并补齐响应预览字段

### 1.1 问题根因
1. `simulate_mock_handler`（`src/web/handlers/mock.rs`）用 `r.path == req.path` 找路由并兜底 `&routes[0]`，**未走 `TrieRouteMatcher`**，导致 `:id`/`{id}` 无法捕获、错误路径误命中。
2. `MockCompiler` 直接取 `req.url.clone()`，当双角色文档写为 `GET {{base_url}}/api/orders/:id?status=done` 或 `GET http://localhost:9000/api/orders/{id}` 时，前端传入 `/api/orders/1` 会匹配失败；且 `src/mock/server.rs` 的 `extract_path` 使用 `url::Url::parse` 会将 `{id}` 百分号编码为 `%7Bid%7D`。
3. 缺少 `@mock-default` 且条件均未命中时，原逻辑仍返回 `matched_variant_name: "default", expected_status: 200`（假阳性，真实服务返回 404）。

### 1.2 统一词法路径提取（`src/mock/compiler.rs` & `src/mock/server.rs`）
在 `src/mock/compiler.rs` 中新增纯词法提取函数 `pub fn extract_route_path(raw_url: &str) -> String`（**禁止使用 `url::Url::parse` 以免将 `{id}` 编码为 `%7Bid%7D`**）：
- 剥离 `?query` 与 `#fragment`；
- 若以 `{{...}}` 变量占位符开头（如 `{{base_url}}/api/users/:id`），剥离该占位符前缀；
- 若含 `scheme://host`（如 `http://localhost:9000/api/users/{id}`），剥离到首个路径 `/`（无路径则回退 `"/"`）；
- 确保返回值始终以 `/` 开头。
- `MockCompiler::compile_inner` 改为 `let path = Self::extract_route_path(&req.url);`；`src/mock/server.rs` 删除私有 `extract_path`，直接复用 `MockCompiler::extract_route_path`。

### 1.3 数据结构变更（`src/web/types.rs` & 既有测试同步）
```rust
// MockSimulateRequest 增加 #[derive(Default)] 与可选字段：
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MockSimulateRequest {
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub query: HashMap<String, String>,
    #[serde(default)]
    pub body: Option<String>,
    pub raw_block: String,
    #[serde(default)]
    pub file_path: Option<String>,
}

// MockSimulateResponse 新增字段（全部 #[serde(default)]）：
#[serde(default)]
pub matched_pattern: Option<String>,
#[serde(default)]
pub captured_params: HashMap<String, String>,
#[serde(default)]
pub response_headers: HashMap<String, String>,
#[serde(default)]
pub response_body: Option<String>,
```
> **必改既有构造点**：`tests/web_api_integration_test.rs` 第 322 行与第 351 行的 `MockSimulateRequest { ... }` 字面量需补充 `file_path: None`（或 `..Default::default()`）。

### 1.4 核心实现步骤
1. **解析入口对齐**（`src/web/handlers/mock.rs`）：
   - 若 `req.file_path` 为 `Some(path)`，调用 `parse_document_by_path(path, &req.raw_block)`；
   - 否则保持现有逻辑（含 ```` ``` ```` 直接走 `MarkdownFileParser::parse_content`，不含则包裹为 ```` ```http\n...\n``` ````）。
2. **路径匹配走 `TrieRouteMatcher`**：
   - 在 `src/mock/matcher.rs` 为 `TrieRouteMatcher` 增加只读方法：
     ```rust
     pub fn match_route_with_params(
         &self,
         method: &str,
         path: &str,
     ) -> Option<(&str, &[MockVariant], HashMap<String, String>)>
     ```
   - `simulate_mock_handler` 用 `MockCompiler::try_compile(&parsed_file)?` 编译路由并构建临时 `TrieRouteMatcher`，调用 `match_route_with_params(&req.method, &req.path)`。
   - **未命中路径或所有变体均未命中时**：`matched_variant_index: None`、`matched_variant_name: "unmatched"`、`expected_status: 404`、`response_headers: HashMap::new()`、`response_body: None`。
3. **响应渲染**：首个命中变体时，将 `captured_params` 写入 `VariableContext`，用 `VariableResolver::resolve` 渲染 `response_body` 与 `response_headers` 返回。
4. **前端展示对齐**（`src/web/assets/app.js` 第 1754–1761 行与第 1834–1842 行）：
   - `runMockSimulate` 请求体补充 `file_path: mockDraft.file_path || null`；
   - `renderMockTab` 根据 `simResult.matched_variant_index !== null && simResult.matched_variant_index !== undefined` 区分 `[MATCHED]` 与 `[UNMATCHED]`，并在结果区展示 `captured_params` 与 `response_body` 预览。

### 1.5 Red 测试与验收
- 测试位置：`src/web/handlers/mock.rs` 内 `mod tests` 及 `tests/web_api_integration_test.rs`。
- 必测用例：
  1. `GET {{base_url}}/api/users/:id?debug=1` 仿真请求 `/api/users/42`，断言 `matched_pattern == Some("/api/users/:id")`、`captured_params["id"] == "42"`、`response_body` 中 `{{id}}` 已替换为 `"42"`；
  2. 仿真请求 `/api/not-exist`，断言 `matched_pattern.is_none()`、`matched_variant_name == "unmatched"`、`expected_status == 404`；
  3. 仅含 `@mock-when` 且条件不满足时，断言 `matched_variant_index.is_none()`、`expected_status == 404`；
  4. 传入 `file_path: Some("api.http")` 的非围栏 `.http` 代码块，断言正常解析。
- 验收命令：`cargo test --lib web::handlers::mock && cargo test --lib mock:: && cargo check --bins --tests`
- 提交：`jj commit -m "fix(web,mock): route mock simulate through TrieRouteMatcher with lexical path extraction"`

---

## Task 2 (P1 · US-2)：Web Mock 服务原地热重载（`POST /api/v1/mock/reload`）与变体计数

### 2.1 设计原则（最小侵入）
**不修改 `AxumMockServer` 或 `serve_matcher_with_events` 签名**。在 `src/mock/matcher.rs` 新增：
```rust
/// 支持运行时原子替换内部 Matcher 的线程安全包装器（用于 Web 原地热重载）
pub struct SwappableMatcher {
    inner: RwLock<Arc<dyn MockMatcher>>,
}
```
为 `SwappableMatcher` 实现 `MockMatcher`（读锁内转发 `match_request(req)`），提供 `pub fn new(initial: Arc<dyn MockMatcher>) -> Self` 与 `pub fn swap(&self, next: Arc<dyn MockMatcher>)`。

### 2.2 上游卡片与 DTO 同步（`src/web/types.rs`、`src/web/http_parse.rs`、`src/web/mock_manager.rs`）
1. `HttpCardDto`（`src/web/types.rs:208`）新增 `#[serde(default)] pub mock_variant_count: usize`；
   - **唯一构造点**：`src/web/http_parse.rs:61`（`parsed_request_to_card_dto`，`.http` 与 `.md` 共用），直接填入 `mock_variant_count: req.metadata.mock_variants.len()`。
2. `MockRouteItemDto`（`src/web/types.rs:229`）新增 `#[serde(default)] pub variant_count: usize`，在 `src/web/mock_manager.rs:275` 填入 `variant_count: card.mock_variant_count`。
3. 新增 `ReloadMockRequest { id: Option<String>, port: Option<u16> }`，并在 `src/web/server.rs` 注册 `.route("/mock/reload", post(handlers::mock::reload_mock_server_handler))`。

### 2.3 `WebMockManager::reload_server` 契约（`src/web/mock_manager.rs`）
1. `RunningMockEntry` 持有 `swappable: Arc<SwappableMatcher>`（`start_server` 中用 `Arc::new(SwappableMatcher::new(Arc::new(matcher)))` 传入 `serve_matcher_with_events`）。
2. `reload_server(&self, working_dir: &Path, id: Option<&str>, port: Option<u16>) -> Result<MockServerInfo, MockManagerError>`：
   - 先持短锁查找目标条目，取出 `file_path` 与 `swappable` 的克隆后**立即释放锁**；
   - 在锁外校验 `PathSandbox` 并调用 `build_mock_matcher` 编译新路由；若编译失败或 `route_count == 0`，**直接返回 `400 Bad Request`，不替换旧 `Matcher`，原端口服务继续稳定运行**；
   - 编译成功后调用 `swappable.swap(Arc::new(new_matcher))`，再重新加锁更新 `entry.info.route_count` 并返回最新 `MockServerInfo`。
3. `reload_mock_server_handler` 首行校验 `if state.readonly { return 403; }`。

### 2.4 Red 测试与验收
- 测试用例：启动 Mock（返回 `"v1"`）→ 改写磁盘文件为 `"v2"` → 调用 `reload_server` → 同一端口请求返回 `"v2"`；再改写为非法 `@mock-when invalid` → 调用 `reload_server` 返回错误 → 同一端口请求仍稳定返回 `"v2"`。
- 验收命令：`cargo test --lib web::mock_manager && cargo check --bins --tests`
- 提交：`jj commit -m "feat(web,mock): add SwappableMatcher and POST /api/v1/mock/reload"`

---

## Task 3 (P1 · US-3)：Web 托管 Mock 实时流量观测（`GET /api/v1/mock/traffic`）

### 3.1 数据结构（`src/web/types.rs`）
```rust
// MockServerInfo 新增：
#[serde(default)]
pub request_count: u64,

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockTrafficItemDto {
    pub method: String,
    pub path: String,
    pub status: u16,
    pub matched_pattern: Option<String>,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockTrafficResponse {
    pub id: String,
    pub port: u16,
    pub total_requests: u64,
    pub events: Vec<MockTrafficItemDto>,
}
```
> **注意**：需同步检查 `src/web/mock_manager.rs:152` 构造 `MockServerInfo` 时初始化 `request_count: 0`。

### 3.2 核心实现与防泄漏约束（`src/web/mock_manager.rs`、`src/web/handlers/mock.rs`、`src/web/server.rs`）
1. `RunningMockEntry` 新增：
   - `traffic: Arc<Mutex<VecDeque<MockTrafficItemDto>>>`（上限 200 条）
   - `request_count: Arc<AtomicU64>`
2. **防任务泄漏铁律**：`let (ev_tx, mut ev_rx) = unbounded_channel::<MockTrafficEvent>();` 中，**`ev_tx` 只能移入 `serve_matcher_with_events`，严禁保存在 `RunningMockEntry` 或消费协程闭包中**！这样当服务停止/被 `Drop` 时，所有 sender 自动销毁，`ev_rx.recv().await` 返回 `None`，消费协程干净退出。
3. `list_servers` 读取 `request_count.load(Ordering::Relaxed)` 填充 `MockServerInfo.request_count`。
4. 注册 `GET /api/v1/mock/traffic`（Query 参数 `id: Option<String>, port: Option<u16>`），返回 `MockTrafficResponse`（只读模式下**允许查询**）。

### 3.3 Red 测试与验收
- 测试用例：启动托管 Mock → 发 1 次 200 请求 + 1 次 404 请求 → 校验 `list_servers` 中 `request_count == 2` → 校验 `get_traffic` 返回 2 条对应记录 → `stop_server` 后确认服务正常释放。
- 验收命令：`cargo test --lib web::mock_manager && cargo check --bins --tests`
- 提交：`jj commit -m "feat(web,mock): expose GET /api/v1/mock/traffic and request_count"`

---

## Task 4 (P2 · US-4/US-5 轻量收口，按需执行)

1. **`RunSuiteRequest` 补充筛选与覆盖**：增加 `#[serde(default)] pub extra_variables: Option<HashMap<String, String>>` 与 `#[serde(default)] pub selector: Option<String>`；`SuiteItemResult` 增加 `#[serde(default)] pub source_line: Option<usize>`。
2. **历史详情端点**：
   - 在 `src/web/server.rs` 注册 `.route("/history/{id}", get(handlers::history::get_history_detail_handler))`（**必须是 `{id}`，禁止 `:id`**）；
   - 从当前工作区隔离后的历史记录中按 `id` 查找，找到后**必须先经 `mask_snapshot_entry` 脱敏**再返回，禁止向浏览器返回未脱敏的历史凭据。

---

## 最终回归门禁（全部 Task 完成后执行）

```bash
cargo test --lib mock::
cargo test --lib web::
cargo test --test mock_integration_test
cargo test --test web_api_integration_test
cargo check --bins --tests
```
全部通过且零编译告警后，更新 `doc/plans/progress_summary.md` 与 `checkpoint.md`。
