//! 变量捕获域专项测试（§5.4 缺口域）。
//!
//! 锁定的行为契约：
//! 1. 逐条降级：一坏一好的 `@capture` 组合，好的照常生效、坏的只进
//!    `TestResult.warnings`（不影响 success 判定、不影响其余捕获）；
//! 2. 变量名含 `from` 的解析：`@capture from_id from body.id` 的关键词
//!    定位必须跳过变量名（此前 `content.find("from")` 会命中变量名内部
//!    的子串导致源表达式错位）；
//! 3. regex 源内含 `from` 子串：`@capture m from regex "from=(\\d+)"`，
//!    源表达式中的 `from` 不得与关键词混淆；捕获组优先于整体匹配；
//! 4. `global.` 前缀跨文件可见性：`@capture global.x` 落入 Arc 共享的
//!    全局罐——依赖链 worker 的 Context 克隆共享同一罐，即使变量差集
//!    克隆机制（`variables()`）不携带 global.* 键，后继文件仍可见。
use rupost::parser::HttpFileParser;
use rupost::runner::{BatchExecutor, BatchMode, BatchRunRequest, TestExecutor};
use rupost::variable::VariableContext;
use std::collections::HashMap;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// 契约 1：逐条降级——好的生效、坏的进 warnings 且不影响 success
#[tokio::test]
async fn test_capture_partial_failure_degrades_per_item() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": "v-1",
            "id": 42
        })))
        .mount(&server)
        .await;

    let temp = tempfile::TempDir::new().unwrap();
    let file = temp.path().join("cap.http");
    std::fs::write(
        &file,
        format!(
            "### Partial captures\n@assert status == 200\n@capture good from body.ok\n@capture bad from body.missing\nGET {}/user\n",
            server.uri()
        ),
    )
    .unwrap();

    let parsed = HttpFileParser::parse_file(&file).unwrap();
    let executor = TestExecutor::new();
    let mut context = VariableContext::new();
    let results = executor.execute_all(parsed, &mut context).await.unwrap();

    let res = &results[0];
    // 逐条降级核心：坏的捕获不影响 success 判定
    assert!(res.success, "capture failure must not fail the request");
    // 好的照常生效
    assert_eq!(context.get("good").as_deref(), Some("v-1"));
    // 坏的不得静默：必须进 warnings 且指名道姓
    assert!(
        res.warnings
            .iter()
            .any(|w| w.contains("bad") && w.contains("capture")),
        "failed capture must surface in warnings: {:?}",
        res.warnings
    );
    // 坏的变量不得以半成品形态存在
    assert_eq!(context.get("bad"), None);
}

/// 契约 2：变量名含 `from`（`from_id`）与 regex 源内含 `from` 子串
#[tokio::test]
async fn test_capture_name_and_regex_containing_from() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/auth"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({
                    "session": "s-1",
                    "note": "from=98765"
                }))
                .insert_header("X-Request-Id", "req-from-77"),
        )
        .mount(&server)
        .await;

    let temp = tempfile::TempDir::new().unwrap();
    let file = temp.path().join("from.http");
    std::fs::write(
        &file,
        format!(
            "### From-ish captures\n@assert status == 200\n@capture from_id from body.session\n@capture m from regex \"from=(\\d+)\"\n@capture rid from header.X-Request-Id\nGET {}/auth\n",
            server.uri()
        ),
    )
    .unwrap();

    let parsed = HttpFileParser::parse_file(&file).unwrap();
    let executor = TestExecutor::new();
    let mut context = VariableContext::new();
    let results = executor.execute_all(parsed, &mut context).await.unwrap();

    assert!(
        results[0].success,
        "all three captures must parse and succeed: {:?}",
        results[0].warnings
    );
    // 变量名 from_id：关键词定位跳过变量名，源 body.session 正确解析
    assert_eq!(context.get("from_id").as_deref(), Some("s-1"));
    // regex 源内含 from：正则在原始 JSON 文本上匹配，捕获组（\d+）优先于整体匹配
    assert_eq!(context.get("m").as_deref(), Some("98765"));
    // header 源不受影响
    assert_eq!(context.get("rid").as_deref(), Some("req-from-77"));
}

/// 契约 3：`global.` 前缀跨文件克隆后的可见性（并行 DAG 依赖链）
#[tokio::test]
async fn test_global_capture_visible_across_files_via_shared_store() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "token": "g-secret",
            "local": "l-1"
        })))
        .mount(&server)
        .await;
    // 文件 B 用 global 变量拼 URL：可见则命中 /g/g-secret
    Mock::given(method("GET"))
        .and(path("/g/g-secret"))
        .respond_with(ResponseTemplate::new(200).set_body_string("global-ok"))
        .mount(&server)
        .await;

    let temp = tempfile::TempDir::new().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let file_a = root.join("a.http");
    let file_b = root.join("b.http");
    std::fs::write(
        &file_a,
        format!(
            "### A\n# @depends-on b.http\n@capture global.g_token from body.token\n@capture local_only from body.local\nGET {}/token\n",
            server.uri()
        ),
    )
    .unwrap();
    std::fs::write(
        &file_b,
        format!("### B\nGET {}/g/{{{{global.g_token}}}}\n", server.uri()),
    )
    .unwrap();

    let mut files_map = HashMap::new();
    files_map.insert(file_a.clone(), HttpFileParser::parse_file(&file_a).unwrap());
    files_map.insert(file_b.clone(), HttpFileParser::parse_file(&file_b).unwrap());

    // 并行模式：B 的 worker context 是克隆——global 罐经 Arc 共享必须仍然可见。
    let executor = TestExecutor::new();
    let batch = BatchExecutor::new(executor);
    let mut context = VariableContext::new();
    let results = batch
        .execute_batch(BatchRunRequest {
            execution_order: vec![file_b.clone(), file_a.clone()],
            dependencies: HashMap::from([(file_b.clone(), vec![file_a.clone()])]),
            files_map,
            context: &mut context,
            mode: BatchMode::Parallel,
            concurrency: 2,
            fail_fast: false,
        })
        .await
        .unwrap();

    let b_results = &results.iter().find(|(p, _)| p == &file_b).unwrap().1;
    assert!(
        b_results.iter().all(|r| r.success),
        "file B must see global.g_token captured by file A through the shared store: {:?}",
        b_results[0].error
    );
}
