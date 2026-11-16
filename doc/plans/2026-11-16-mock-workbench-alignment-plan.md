# Mock 与 Web 工作台对齐实施计划（Flash 级可执行规范）

> **目标**：打通 US-1 ~ US-5 在 Web 工作台与 Mock 核心引擎之间的断点，消除仿真器双脑裂、补齐原地热重载与实时流量观测能力。  
> **前置基础**：`TrieNode::upsert`、`EvalContext`、`MockTrafficEvent`、`serve_matcher_with_events` 与 `MockCompiler::try_compile` 已在主干就绪，本计划直接复用，**禁止重复造轮子**。

---

## 0. 执行铁律（接手模型必读）

1. **Clean Imports**：禁止在函数体、结构体初始化或测试代码内书写 3 段及以上全限定路径（如 `std::sync::Arc::new`、`crate::mock::...`），统一在文件顶部或 `mod tests` 顶部 `use` 引入（仅 `std::io::Result` 消歧除外）。
2. **向后兼容**：
   - 所有新增到现有 Request/Response DTO（`src/web/types.rs`）的字段，**必须标注 `#[serde(default)]`**，确保旧客户端或既有测试 JSON 反序列化不报错。
3. **只读守卫**：任何修改状态或启停服务端的 Web Handler，必须在首行检查 `if state.readonly { return (StatusCode::FORBIDDEN, ...); }`。
4. **TDD 三步法**：每个 Task 必须先写 Red 测试并确认失败，再写最小实现变绿，最后跑全量相关测试与 `cargo check --bins --tests`（零编译告警）。
5. **原子化提交**：每完成一个 Task，执行一次 `jj commit -m "..."`。

---

## Task 1 (P0 · US-1)：修复 `MockSimulate` 双脑裂并补齐响应预览字段

### 1.1 问题根因
当前 `POST /api/v1/mock/simulate`（`src/web/handlers/mock.rs`）存在两个断点：
1. 用 `r.path == req.path` 找路由并兜底 `&routes[0]`，**完全没走 `TrieRouteMatcher`**，导致 `:id` / `{id}` 路径参数无法捕获、错误路径也会误命中。
2. `MockSimulateResponse` 只返回匹配索引与条件评估，缺少渲染后的响应头与响应体。

### 1.2 数据结构变更（`src/web/types.rs`）
```rust
// MockSimulateRequest 新增可选字段：
#[serde(default)]
pub file_path: Option<String>,

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

### 1.3 核心实现步骤
1. **解析入口对齐**：在 `simulate_mock_handler` 中：
   - 若 `req.file_path` 为 `Some(path)`，调用 `crate::parser::parse_document_by_path(path, &req.raw_block)`；
   - 否则保持现有逻辑（含 ```` ``` ```` 直接走 `MarkdownFileParser::parse_content`，不含则包裹为 ```` ```http\n...\n``` ````）。
2. **路径匹配走 `TrieNode`**：
   - 在 `src/mock/matcher.rs` 为 `TrieRouteMatcher` 增加只读辅助方法（供 `simulate_mock_handler` 复用，避免重复造路由树）：
     ```rust
     pub fn match_route_with_params(
         &self,
         method: &str,
         path: &str,
     ) -> Option<(&str, &[MockVariant], HashMap<String, String>)>
     ```
   - `simulate_mock_handler` 编译 `routes = MockCompiler::try_compile(&parsed_file)?`，构建临时 `TrieRouteMatcher`（调用 `add_route`），再调用 `match_route_with_params(&req.method, &req.path)`。
   - **未命中路径时**：直接返回 `200 OK`，`MockSimulateResponse` 中 `matched_variant_index: None`、`matched_variant_name: "unmatched"`、`expected_status: 404`、`matched_pattern: None`、`captured_params: HashMap::new()`、`response_headers: HashMap::new()`、`response_body: None`、`evaluations: vec![]`。
3. **变体评估与响应渲染**：
   - 命中路由后，构建 `EvalContext::new(&req.headers, &req.query, body_str)` 遍历 `variants` 调用 `cond.evaluate_with_detail(&eval_ctx)` 生成 `evaluations`。
   - 对首个命中的变体，将 `captured_params` 写入 `VariableContext`，用 `VariableResolver::resolve` 渲染 `response_body` 与 `response_headers` 后填入 `MockSimulateResponse`。

### 1.4 Red 测试与验收
- 测试位置：`src/web/handlers/mock.rs` 内 `mod tests`（或 `tests/web_integration_test.rs`）。
- 必须覆盖 3 个用例：
  1. 定义 `GET /api/users/:id`，仿真请求 `/api/users/42`，断言 `matched_pattern == Some("/api/users/:id")`、`captured_params["id"] == "42"`、`response_body` 中 `{{id}}` 已替换为 `"42"`；
  2. 仿真请求 `/api/not-exist`，断言 `matched_pattern.is_none()`、`expected_status == 404`；
  3. 传入 `.http` 风格 `file_path: Some("api.http")` 的非围栏代码块，断言正常解析与仿真。
- 验收命令：`cargo test --lib web::handlers::mock` & `cargo test --lib mock::`
- 提交：`jj commit -m "fix(web,mock): route mock simulate through TrieRouteMatcher and return rendered response"`

---

## Task 2 (P1 · US-2)：Web Mock 服务原地热重载（`POST /api/v1/mock/reload`）

### 2.1 设计原则（最小侵入）
**不要修改 `AxumMockServer` 或 `serve_matcher_with_events` 的签名**。只需在 `src/mock/matcher.rs` 新增一个轻量包装器：
```rust
/// 支持运行时原子替换内部 Matcher 的线程安全包装器（用于 Web 原地热重载）
pub struct SwappableMatcher {
    inner: std::sync::RwLock<Arc<dyn MockMatcher>>,
}
```
为 `SwappableMatcher` 实现 `MockMatcher` trait（读锁内调用 `guard.match_request(req)`），并提供 `pub fn swap(&self, next: Arc<dyn MockMatcher>)`。

### 2.2 数据结构与路由（`src/web/types.rs` & `src/web/server.rs`）
1. `MockRouteItemDto` 新增：`#[serde(default)] pub variant_count: usize`（在 `discover_workspace_mock_routes` 中填充 `variants.len()`）。
2. 新增 `ReloadMockRequest`（`id: Option<String>, port: Option<u16>`，逻辑与 `StopMockRequest` 相同）。
3. `src/web/server.rs` 注册：`.route("/mock/reload", post(handlers::mock::reload_mock_server_handler))`。

### 2.3 `WebMockManager::reload_server` 契约（`src/web/mock_manager.rs`）
1. `RunningMockEntry` 持有 `swappable: Arc<SwappableMatcher>`（启动时用 `SwappableMatcher::new(Arc::new(matcher))` 传给 `serve_matcher_with_events`）。
2. `reload_server(&self, working_dir: &Path, id: Option<&str>, port: Option<u16>) -> Result<MockServerInfo, MockManagerError>`：
   - 根据 `id` 或 `port` 定位运行中的 `RunningMockEntry`，拿到 `file_path` 与 `swappable`；
   - **先在锁外**重新校验路径沙箱并调用 `build_mock_matcher` 编译新路由（若新文件有语法错误或 `route_count == 0`，**直接返回 `400 Bad Request`，不替换旧 `Matcher`，原端口服务不中断**）；
   - 编译成功后调用 `swappable.swap(Arc::new(new_matcher))`，更新 `entry.info.route_count`，返回最新 `MockServerInfo`。
3. `reload_mock_server_handler` 必须检查 `if state.readonly { return 403; }`。

### 2.4 Red 测试与验收
- 测试用例：启动 Mock（返回 `"v1"`）→ 改写磁盘文件为 `"v2"` → 调用 `reload_server` → 同一端口请求返回 `"v2"`；再改写为非法 `@mock-when invalid` → 调用 `reload_server` 返回错误 → 同一端口请求仍稳定返回 `"v2"`。
- 验收命令：`cargo test --lib web::mock_manager`
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

### 3.2 核心实现（`src/web/mock_manager.rs` & `src/web/handlers/mock.rs`）
1. `RunningMockEntry` 新增：
   - `traffic: Arc<Mutex<VecDeque<MockTrafficEvent>>>`（上限 200 条）
   - `request_count: Arc<AtomicU64>`
2. `start_server` 中创建 `let (ev_tx, mut ev_rx) = unbounded_channel::<MockTrafficEvent>();`，传 `Some(ev_tx)` 给 `serve_matcher_with_events`；spawn 消费任务每次收到事件时原子 `+1` 并写入有界 `VecDeque`（超出 200 条 `pop_front`）。
3. `list_servers` 读取 `request_count.load(Ordering::Relaxed)` 填充 `MockServerInfo.request_count`。
4. 新增 `GET /api/v1/mock/traffic`（Query 参数 `id: Option<String>, port: Option<u16>`），返回 `MockTrafficResponse`（只读模式下**允许查询**）。

### 3.3 Red 测试与验收
- 测试用例：启动托管 Mock → 发 1 次 200 请求 + 1 次 404 请求 → 校验 `list_servers` 中 `request_count == 2` → 校验 `get_traffic` 返回 2 条对应记录。
- 验收命令：`cargo test --lib web::mock_manager` & `cargo check --bins --tests`
- 提交：`jj commit -m "feat(web,mock): expose GET /api/v1/mock/traffic and request_count"`

---

## Task 4 (P2 · US-4/US-5 轻量收口，按需执行)

1. **`RunSuiteRequest` 补充筛选与覆盖**：增加 `#[serde(default)] pub extra_variables: Option<HashMap<String, String>>` 与 `#[serde(default)] pub selector: Option<String>`；`SuiteItemResult` 增加 `#[serde(default)] pub source_line: Option<usize>`。
2. **历史详情端点**：新增 `GET /api/v1/history/{id}`，从工作区隔离后的历史记录中按 `id` 查找并返回脱敏后的请求/响应详情（仅允许返回属于当前工作区的条目，复用 `history` handler 现有工作区隔离逻辑）。

---

## 最终回归门禁（全部 Task 完成后执行）

```bash
cargo test --lib mock::
cargo test --lib web::
cargo test --test mock_integration_test
cargo check --bins --tests
```
全部通过且零编译告警后，更新 `doc/plans/progress_summary.md` 与 `checkpoint.md`。
