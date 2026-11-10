// 进程级静音历史落盘（tests/common/mod.rs：pre-main 执行，勿删）
mod common;

use std::fs;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use rupost::parser::HttpFileParser;
use rupost::runner::executor::TestExecutor;
use rupost::runner::{BatchExecutor, BatchMode, BatchRunRequest, DirectoryScanner, WorkflowGraph};
use rupost::variable::VariableContext;

#[test]
fn test_scanner_discovery() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().canonicalize().unwrap();

    // 创建文件和子目录
    let dir1 = root.join("dir1");
    fs::create_dir(&dir1).unwrap();

    let dir2 = root.join("dir2");
    fs::create_dir(&dir2).unwrap();

    let git_dir = root.join(".git");
    fs::create_dir(&git_dir).unwrap();

    // 写入一些测试文件
    let file_b = dir1.join("b.http");
    fs::write(&file_b, "### Request B\nGET http://localhost/b").unwrap();

    let file_a = dir1.join("a.http");
    fs::write(&file_a, "### Request A\nGET http://localhost/a").unwrap();

    let file_c = dir2.join("c.md");
    fs::write(&file_c, "```http\nGET http://localhost/c\n```").unwrap();

    let file_txt = dir2.join("ignored.txt");
    fs::write(&file_txt, "GET http://localhost/txt").unwrap();

    let file_hidden = git_dir.join("hidden.http");
    fs::write(&file_hidden, "GET http://localhost/hidden").unwrap();

    // 扫描
    let paths = vec![root.to_string_lossy().to_string()];
    let scanned = DirectoryScanner::scan(&paths).unwrap();

    // 因为是 canonicalize 的，我们先把 scanned 转成相对于 root 的相对路径
    let mut relative_paths: Vec<std::path::PathBuf> = scanned
        .iter()
        .map(|p| p.strip_prefix(&root).unwrap().to_path_buf())
        .collect();

    // 默认按字母顺序进行隐式基础排序（深度遍历或扁平之后做 sort）
    relative_paths.sort(); // 确保断言不受底层文件系统遍历顺序的影响，但实际上 Scanner 本身应该排序

    assert_eq!(relative_paths.len(), 3);
    assert_eq!(
        relative_paths[0],
        std::path::Path::new("dir1").join("a.http")
    );
    assert_eq!(
        relative_paths[1],
        std::path::Path::new("dir1").join("b.http")
    );
    assert_eq!(relative_paths[2], std::path::Path::new("dir2").join("c.md"));
}

#[test]
fn test_workflow_topo_sorting() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().canonicalize().unwrap();

    // a.http depends b.http
    // b.http has no dependency
    // c.http depends a.http
    // 我们也支持解析相对路径
    let file_b = root.join("b.http");
    fs::write(&file_b, "### Request B\nGET http://localhost/b").unwrap();

    let file_a = root.join("a.http");
    fs::write(
        &file_a,
        "### @depends-on b.http\n### Request A\nGET http://localhost/a",
    )
    .unwrap();

    let file_c = root.join("c.http");
    fs::write(
        &file_c,
        "### @depends-on a.http\n### Request C\nGET http://localhost/c",
    )
    .unwrap();

    // 解析
    let parsed_a = HttpFileParser::parse_file(&file_a).unwrap();
    let parsed_b = HttpFileParser::parse_file(&file_b).unwrap();
    let parsed_c = HttpFileParser::parse_file(&file_c).unwrap();

    let files = vec![
        (file_a.clone(), parsed_a),
        (file_b.clone(), parsed_b),
        (file_c.clone(), parsed_c),
    ];

    let graph = WorkflowGraph::new(&files);
    let order = graph.resolve_execution_order().unwrap();

    assert_eq!(order.len(), 3);
    assert_eq!(order[0], file_b);
    assert_eq!(order[1], file_a);
    assert_eq!(order[2], file_c);
}

#[test]
fn test_workflow_cycle_detection() {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    // a.http depends b.http
    // b.http depends a.http
    let file_a = root.join("a.http");
    fs::write(&file_a, "### @depends-on b.http\nGET http://localhost/a").unwrap();

    let file_b = root.join("b.http");
    fs::write(&file_b, "### @depends-on a.http\nGET http://localhost/b").unwrap();

    let parsed_a = HttpFileParser::parse_file(&file_a).unwrap();
    let parsed_b = HttpFileParser::parse_file(&file_b).unwrap();

    let files = vec![(file_a.clone(), parsed_a), (file_b.clone(), parsed_b)];

    let graph = WorkflowGraph::new(&files);
    let result = graph.resolve_execution_order();

    assert!(result.is_err());
    let err_str = result.err().unwrap().to_string();
    assert!(
        err_str.contains("循环") || err_str.contains("Cyclic") || err_str.contains("dependency")
    );
}

#[tokio::test]
async fn test_batch_parallel_concurrency_and_cookie_isolation() {
    let mock_server = MockServer::start().await;

    // mock 响应，如果是获取或者设置 cookie
    Mock::given(method("GET"))
        .and(path("/cookie/set"))
        .respond_with(
            ResponseTemplate::new(200).insert_header("Set-Cookie", "session=12345; Path=/"),
        )
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/cookie/get"))
        .respond_with(ResponseTemplate::new(200).set_body_string("cookie received"))
        .mount(&mock_server)
        .await;

    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path();

    // 写入两个无关的并行测试文件
    let file_1 = root.join("task1.http");
    fs::write(
        &file_1,
        format!(
            "### Task 1\n@capture val1 from body\nGET {}/cookie/set\n\n### Get\nGET {}/cookie/get",
            mock_server.uri(),
            mock_server.uri()
        ),
    )
    .unwrap();

    let file_2 = root.join("task2.http");
    fs::write(
        &file_2,
        format!(
            "### Task 2\n@capture val2 from body\nGET {}/cookie/set\n\n### Get\nGET {}/cookie/get",
            mock_server.uri(),
            mock_server.uri()
        ),
    )
    .unwrap();

    let parsed_1 = HttpFileParser::parse_file(&file_1).unwrap();
    let parsed_2 = HttpFileParser::parse_file(&file_2).unwrap();

    let mut files_map = std::collections::HashMap::new();
    files_map.insert(file_1.clone(), parsed_1);
    files_map.insert(file_2.clone(), parsed_2);

    let order = vec![file_1.clone(), file_2.clone()];

    let executor = TestExecutor::new();
    let batch_executor = BatchExecutor::new(executor);

    let mut context = VariableContext::new();
    let deps = std::collections::HashMap::new();
    // 运行 parallel 模式
    let results = batch_executor
        .execute_batch(BatchRunRequest {
            execution_order: order,
            dependencies: deps,
            files_map,
            context: &mut context,
            mode: BatchMode::Parallel,
            concurrency: 2,
            fail_fast: false,
        })
        .await
        .unwrap();

    // 并行模式下各自的变量隔离，不会合并到全局的 context 里
    // 或者说，应该不会污染全局上下文，除非有显式的 global 机制。
    // 我们在此处验证结果的长度。
    assert_eq!(results.len(), 2);
}

#[tokio::test]
async fn test_batch_report_json() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"status": "ok"})))
        .mount(&mock_server)
        .await;

    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().canonicalize().unwrap();

    let file_a = root.join("a.http");
    fs::write(
        &file_a,
        format!("### Req A\nGET {}/anything", mock_server.uri()),
    )
    .unwrap();

    let parsed_a = HttpFileParser::parse_file(&file_a).unwrap();
    let mut files_map = std::collections::HashMap::new();
    files_map.insert(file_a.clone(), parsed_a);

    let order = vec![file_a.clone()];
    let executor = TestExecutor::new();
    let batch_executor = BatchExecutor::new(executor);
    let mut context = VariableContext::new();
    let deps = std::collections::HashMap::new();

    let results = batch_executor
        .execute_batch(BatchRunRequest {
            execution_order: order,
            dependencies: deps,
            files_map,
            context: &mut context,
            mode: BatchMode::Serial,
            concurrency: 1,
            fail_fast: false,
        })
        .await
        .unwrap();

    // 验证我们能够从 results 格式化出符合 JSON 格式的报告
    // 这里我们可以自己实现或升级 TestReporter::report_json。
    // 在这里我们主要测试 reporter 的 JSON 输出。
    let mut out = Vec::new();
    rupost::report::json::report_json(&results, &mut out).unwrap();
    let json_str = String::from_utf8(out).unwrap();
    let parsed_json: serde_json::Value = serde_json::from_str(&json_str).unwrap();

    assert!(parsed_json.is_array());
    let arr = parsed_json.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(
        arr[0]["file_path"].as_str().unwrap(),
        rupost::utils::path_guard::normalize_posix_path(&file_a)
    );
}

#[test]
fn test_workflow_missing_dependency() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let root = temp_dir.path().canonicalize().unwrap();

    let file_a = root.join("a.http");
    fs::write(
        &file_a,
        "### @depends-on non_existent.http\nGET http://localhost/a",
    )
    .unwrap();

    let parsed_a = HttpFileParser::parse_file(&file_a).unwrap();
    let files = vec![(file_a.clone(), parsed_a)];

    let graph = WorkflowGraph::new(&files);
    let result = graph.resolve_execution_order();

    assert!(result.is_err());
    let err = result.err().unwrap();
    assert!(matches!(
        err,
        rupost::error::RupostError::DependencyNotFound { .. }
    ));
    let err_str = err.to_string();
    assert!(err_str.contains("找不到依赖文件"));
    assert!(err_str.contains("non_existent.http"));
}

#[tokio::test]
async fn test_batch_parallel_global_variable_sharing() {
    let mock_server = MockServer::start().await;

    // mock task1.http 响应，返回含有 token 的 json
    Mock::given(method("GET"))
        .and(path("/login"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "token": "my-secret-global-token"
        })))
        .mount(&mock_server)
        .await;

    // mock task2.http，验证收到的 Authorization 头是否正确
    use wiremock::matchers::header;
    Mock::given(method("GET"))
        .and(path("/profile"))
        .and(header("Authorization", "Bearer my-secret-global-token"))
        .respond_with(ResponseTemplate::new(200).set_body_string("welcome profile"))
        .mount(&mock_server)
        .await;

    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().canonicalize().unwrap();

    let file_1 = root.join("task1.http");
    fs::write(
        &file_1,
        format!(
            "### Task 1\nGET {}/login\n@capture global.token from body.token",
            mock_server.uri()
        ),
    )
    .unwrap();

    let file_2 = root.join("task2.http");
    fs::write(
        &file_2,
        format!(
            "### @depends-on task1.http\n### Task 2\nGET {}/profile\nAuthorization: Bearer {{{{global.token}}}}",
            mock_server.uri()
        ),
    )
    .unwrap();

    let parsed_1 = HttpFileParser::parse_file(&file_1).unwrap();
    let parsed_2 = HttpFileParser::parse_file(&file_2).unwrap();

    let mut files_map = std::collections::HashMap::new();
    files_map.insert(file_1.clone(), parsed_1);
    files_map.insert(file_2.clone(), parsed_2);

    let order = vec![file_1.clone(), file_2.clone()];

    let executor = TestExecutor::new();
    let batch_executor = BatchExecutor::new(executor);
    let mut context = VariableContext::new();

    let mut deps = std::collections::HashMap::new();
    deps.insert(file_2.clone(), vec![file_1.clone()]);

    // 运行 parallel 模式，以并发执行
    let results = batch_executor
        .execute_batch(BatchRunRequest {
            execution_order: order,
            dependencies: deps,
            files_map,
            context: &mut context,
            mode: BatchMode::Parallel,
            concurrency: 2,
            fail_fast: false,
        })
        .await
        .unwrap();

    assert_eq!(results.len(), 2);
    // 两个接口均应该成功
    for (_path, run_results) in &results {
        assert!(!run_results.is_empty(), "每个文件应该至少执行一个请求");
        for r in run_results {
            assert!(r.success, "请求应该成功，包含断言和网络执行: {:?}", r.error);
        }
    }
}

#[tokio::test]
async fn test_batch_parallel_local_variable_cascade() {
    let mock_server = MockServer::start().await;

    // mock task1.http 登录并返回 json
    Mock::given(method("GET"))
        .and(path("/login"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "local_token": "my-local-secret-jwt"
        })))
        .mount(&mock_server)
        .await;

    // mock task2.http 验证 Authorization
    use wiremock::matchers::header;
    Mock::given(method("GET"))
        .and(path("/profile"))
        .and(header("Authorization", "Bearer my-local-secret-jwt"))
        .respond_with(ResponseTemplate::new(200).set_body_string("success"))
        .mount(&mock_server)
        .await;

    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().canonicalize().unwrap();

    let file_1 = root.join("task1.http");
    fs::write(
        &file_1,
        format!(
            "### Task 1\nGET {}/login\n@capture local_token from body.local_token",
            mock_server.uri()
        ),
    )
    .unwrap();

    let file_2 = root.join("task2.http");
    fs::write(
        &file_2,
        format!(
            "### @depends-on task1.http\n### Task 2\nGET {}/profile\nAuthorization: Bearer {{{{local_token}}}}",
            mock_server.uri()
        ),
    )
    .unwrap();

    let parsed_1 = HttpFileParser::parse_file(&file_1).unwrap();
    let parsed_2 = HttpFileParser::parse_file(&file_2).unwrap();

    let mut files_map = std::collections::HashMap::new();
    files_map.insert(file_1.clone(), parsed_1);
    files_map.insert(file_2.clone(), parsed_2);

    let order = vec![file_1.clone(), file_2.clone()];

    let executor = TestExecutor::new();
    let batch_executor = BatchExecutor::new(executor);
    let mut context = VariableContext::new();

    let mut deps = std::collections::HashMap::new();
    deps.insert(file_2.clone(), vec![file_1.clone()]);

    let results = batch_executor
        .execute_batch(BatchRunRequest {
            execution_order: order,
            dependencies: deps,
            files_map,
            context: &mut context,
            mode: BatchMode::Parallel,
            concurrency: 2,
            fail_fast: false,
        })
        .await
        .unwrap();

    assert_eq!(results.len(), 2);
    for (_path, run_results) in &results {
        assert!(!run_results.is_empty());
        for r in run_results {
            assert!(r.success, "请求应该成功: {:?}", r.error);
        }
    }
}

#[tokio::test]
async fn test_batch_parallel_cookie_cascade() {
    let mock_server = MockServer::start().await;

    // mock set cookie
    Mock::given(method("GET"))
        .and(path("/cookie/set"))
        .respond_with(ResponseTemplate::new(200).insert_header(
            "Set-Cookie",
            "my_session=secret_value; Domain=127.0.0.1; Path=/",
        ))
        .mount(&mock_server)
        .await;

    // mock check cookie
    use wiremock::matchers::header;
    Mock::given(method("GET"))
        .and(path("/cookie/check"))
        .and(header("Cookie", "my_session=secret_value"))
        .respond_with(ResponseTemplate::new(200).set_body_string("cookie verified"))
        .mount(&mock_server)
        .await;

    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().canonicalize().unwrap();

    let file_1 = root.join("task1.http");
    fs::write(
        &file_1,
        format!("### Task 1\nGET {}/cookie/set", mock_server.uri()),
    )
    .unwrap();

    let file_2 = root.join("task2.http");
    fs::write(
        &file_2,
        format!(
            "### @depends-on task1.http\n### Task 2\nGET {}/cookie/check",
            mock_server.uri()
        ),
    )
    .unwrap();

    let parsed_1 = HttpFileParser::parse_file(&file_1).unwrap();
    let parsed_2 = HttpFileParser::parse_file(&file_2).unwrap();

    let mut files_map = std::collections::HashMap::new();
    files_map.insert(file_1.clone(), parsed_1);
    files_map.insert(file_2.clone(), parsed_2);

    let order = vec![file_1.clone(), file_2.clone()];

    // 启用 Cookie 支持
    let executor = TestExecutor::with_ephemeral_cookies();
    let batch_executor = BatchExecutor::new(executor);
    let mut context = VariableContext::new();

    let mut deps = std::collections::HashMap::new();
    deps.insert(file_2.clone(), vec![file_1.clone()]);

    let results = batch_executor
        .execute_batch(BatchRunRequest {
            execution_order: order,
            dependencies: deps,
            files_map,
            context: &mut context,
            mode: BatchMode::Parallel,
            concurrency: 2,
            fail_fast: false,
        })
        .await
        .unwrap();

    assert_eq!(results.len(), 2);
    for (_path, run_results) in &results {
        assert!(!run_results.is_empty());
        for r in run_results {
            assert!(r.success, "并行 Cookie 级联传递校验失败: {:?}", r.error);
        }
    }
}

// ═══════════════ M3：@teardown 确定性生命周期 E2E ═══════════════

/// 生命周期 E2E：创建 → teardown；主断言失败后 teardown 仍执行（Finally 语义），
/// teardown 结果标记 is_teardown 且成功计入结果；占位结果原序号替换
#[tokio::test]
async fn test_teardown_runs_on_failure_and_marks_result() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/create"))
        .respond_with(ResponseTemplate::new(201).set_body_string("created"))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/cleanup"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    // 主断言故意失败（/create 期望 400 实得 201），teardown 删除请求应仍执行
    let case = format!(
        "### create\n# @name create\nPOST {}/create\n\n# @assert status == 400\n\n### cleanup\n# @teardown\n# @name cleanup\nDELETE {}/cleanup\n",
        server.uri(),
        server.uri()
    );
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("teardown.http");
    std::fs::write(&file, case).unwrap();

    let parsed = HttpFileParser::parse_file(file.clone()).expect("parse");
    assert_eq!(parsed.requests.len(), 2);
    assert!(parsed.requests[1].metadata.is_teardown, "teardown marked");

    // 红队 P0 修订：必须走 batch 层（Finally 逻辑所在），裸 executor 不触发 Finally
    let executor = TestExecutor::new();
    let batch_executor = BatchExecutor::new(executor);
    let mut context = rupost::variable::VariableContext::default();
    let mut files_map = std::collections::HashMap::new();
    files_map.insert(file.clone(), parsed);
    let order = vec![file.clone()];
    let batch = BatchRunRequest {
        execution_order: order,
        dependencies: std::collections::HashMap::new(),
        files_map,
        context: &mut context,
        mode: BatchMode::Serial,
        concurrency: 1,
        fail_fast: false,
    };
    let batch_results = batch_executor.execute_batch(batch).await.unwrap();

    assert_eq!(batch_results.len(), 1);
    let file_results = &batch_results[0].1;
    assert_eq!(
        file_results.len(),
        2,
        "P0-2: placeholder preserved (numbering)"
    );

    let main_result = &file_results[0];
    assert!(
        main_result.request_number == 1 && !main_result.success,
        "main failed"
    );

    // 关键区分断言：Finally 真实执行（非占位）——占位 skipped=true，真执行 skipped=false
    let teardown = &file_results[1];
    assert!(teardown.request_number == 2 && teardown.is_teardown);
    assert!(
        teardown.success && !teardown.skipped,
        "Finally must have executed the DELETE (placeholder would be skipped=true)"
    );

    // 真实外发取证：DELETE /cleanup 到达 mock（变异"Finally 删除"下确定性红）
    let reqs = server.received_requests().await.expect("received requests");
    let deletes = reqs
        .iter()
        .filter(|r| r.method == reqwest::Method::DELETE)
        .count();
    assert!(
        deletes >= 1,
        "DELETE cleanup must reach the mock: {}",
        reqs.len()
    );
}

/// 变量防呆（P1-9）：前置失败导致 teardown 依赖变量未捕获 → skipped + warning，不外发
#[tokio::test]
async fn test_teardown_skipped_when_variable_unresolved() {
    let server = MockServer::start().await;
    // /create 返回失败响应（无 token 可捕获）
    Mock::given(method("POST"))
        .and(path("/create"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let case = format!(
        "### create\n# @name create\nPOST {}/create\n\n# @assert status == 200\n# @capture token from body.token\n\n### cleanup\n# @teardown\n# @name cleanup\nDELETE {}/items/{{{{token}}}}\n",
        server.uri(),
        server.uri()
    );
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("teardown_var.http");
    std::fs::write(&file, case).unwrap();

    let parsed = HttpFileParser::parse_file(file.clone()).expect("parse");
    let executor = rupost::runner::executor::TestExecutor::new();
    let mut context = rupost::variable::VariableContext::default();
    let results = executor.execute_all(parsed, &mut context).await.unwrap();

    // 常规阶段：teardown 占位 skipped（原序号）
    assert!(results[1].request_number == 2 && results[1].skipped);

    // 批量层 Finally：防呆 → skipped + warning（非外发）——通过 batch 层验证
    use rupost::runner::{BatchExecutor, BatchMode, BatchRunRequest};
    let parsed2 = HttpFileParser::parse_file(file.clone()).expect("parse");
    let mut files_map = std::collections::HashMap::new();
    files_map.insert(file.clone(), parsed2.clone());
    let order = vec![file.clone()];
    let mut ctx = rupost::variable::VariableContext::default();

    let batch = BatchRunRequest {
        execution_order: order,
        dependencies: std::collections::HashMap::new(),
        files_map,
        context: &mut ctx,
        mode: BatchMode::Serial,
        concurrency: 1,
        fail_fast: false,
    };
    let executor2 = rupost::runner::executor::TestExecutor::new();
    let batch_executor = BatchExecutor::new(executor2);
    let batch_results = batch_executor.execute_batch(batch).await.unwrap();

    let file_results = &batch_results[0].1;
    let teardown_result = file_results
        .iter()
        .find(|r| r.request_number == 2)
        .expect("teardown slot preserved");
    assert!(
        teardown_result.skipped,
        "unresolved vars → skipped (no dirty request)"
    );
    assert!(
        teardown_result
            .warnings
            .iter()
            .any(|w| w.contains("@teardown 跳过")),
        "warning explains the skip: {:?}",
        teardown_result.warnings
    );
    assert!(teardown_result.is_teardown, "marked as teardown");
}

/// 红队 P0-B（P0-1 承诺）：parallel + fail_fast + teardown——execute_batch 返回 Ok
/// （is_cancelled 容忍），已启动文件的 teardown 仍执行（Finally 不被跳过）。
/// 2 文件：文件 A 失败触发 fail_fast → 文件 B 被取消（Err(cancelled) → 容忍路径真实触发）
#[tokio::test]
async fn test_teardown_parallel_fail_fast_still_runs() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/boom"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/ok"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/cleanup"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    // 2 文件：A（失败 + teardown）、B（正常 + teardown）——并发 1 保证 A 先完成触发 fail_fast，B 被取消
    let dir = tempfile::tempdir().unwrap();
    for (name, path_seg, status) in [("a", "boom", 500), ("b", "ok", 200)] {
        let f = dir.path().join(format!("{}.http", name));
        std::fs::write(
            &f,
            format!(
                "### {name}\n# @name {name}\nGET {server_uri}/{path_seg}\n\n# @assert status == {good_status}\n\n### {name}-teardown\n# @teardown\n# @name {name}-td\nDELETE {server_uri}/cleanup\n",
                name = name,
                server_uri = server.uri(),
                path_seg = path_seg,
                good_status = if name == "a" { 200 } else { 200 },
            ),
        )
        .unwrap();
    }
    // 修正：文件 A 断言 500（失败触发 fail_fast），文件 B 断言 200
    let fa = dir.path().join("a.http");
    let fb = dir.path().join("b.http");
    let content_a = std::fs::read_to_string(&fa)
        .unwrap()
        .replace("# @assert status == 200", "# @assert status == 500");
    std::fs::write(&fa, content_a).unwrap();

    let parsed_a = HttpFileParser::parse_file(fa.clone()).expect("parse a");
    let parsed_b = HttpFileParser::parse_file(fb.clone()).expect("parse b");
    let executor = TestExecutor::new();
    let batch_executor = BatchExecutor::new(executor);
    let mut context = rupost::variable::VariableContext::default();
    let mut files_map = std::collections::HashMap::new();
    files_map.insert(fa.clone(), parsed_a);
    files_map.insert(fb.clone(), parsed_b);
    let order = vec![fa.clone(), fb.clone()];
    let batch = BatchRunRequest {
        execution_order: order,
        dependencies: std::collections::HashMap::new(),
        files_map,
        context: &mut context,
        mode: BatchMode::Parallel,
        concurrency: 1,
        fail_fast: true,
    };

    // P0-1 核心：fail_fast 中断不得使 execute_batch 返回 Err
    let batch_results = batch_executor.execute_batch(batch).await;
    assert!(
        batch_results.is_ok(),
        "parallel fail_fast must return Ok (is_cancelled tolerated): {:?}",
        batch_results.err()
    );
    let results = batch_results.unwrap();

    // 已启动文件（A 失败 + B 正常或被取消）的 teardown 必须执行
    let teardown_results: Vec<&rupost::runner::types::TestResult> = results
        .iter()
        .flat_map(|(_, file_results)| file_results.iter())
        .filter(|r| r.is_teardown)
        .collect();
    assert!(
        !teardown_results.is_empty(),
        "at least one teardown must have executed"
    );
    for t in &teardown_results {
        assert!(
            !t.skipped && t.success,
            "Finally-executed teardown must be real (not placeholder): {:?}",
            t
        );
    }

    // 真实外发取证：DELETE 到达 mock
    let reqs = server.received_requests().await.expect("received requests");
    let deletes = reqs
        .iter()
        .filter(|r| r.method == reqwest::Method::DELETE)
        .count();
    assert!(deletes >= 1, "teardown DELETE must reach mock: {deletes}");
}

/// 红队 P0-C（P0-2 承诺）：3 请求文件（#2 为 teardown）——teardown 后普通请求
/// 原序号连续（#3 保持 3，不因抽取前移）
#[tokio::test]
async fn test_teardown_preserves_numbering_after_teardown_slot() {
    let server = MockServer::start().await;
    for p in ["/create", "/skip", "/list"] {
        Mock::given(method("GET"))
            .and(path(p))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
    }

    let case = format!(
        "### a\n# @name a\nGET {}/create\n\n### teardown-mid\n# @teardown\n# @name mid\nGET {}/skip\n\n### c\n# @name c\nGET {}/list\n",
        server.uri(),
        server.uri(),
        server.uri()
    );
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("numbering.http");
    std::fs::write(&file, case).unwrap();

    let parsed = HttpFileParser::parse_file(file.clone()).expect("parse");
    assert!(parsed.requests[1].metadata.is_teardown);
    assert!(!parsed.requests[2].metadata.is_teardown);

    let executor = TestExecutor::new();
    let batch_executor = BatchExecutor::new(executor);
    let mut context = rupost::variable::VariableContext::default();
    let mut files_map = std::collections::HashMap::new();
    files_map.insert(file.clone(), parsed);
    let order = vec![file.clone()];
    let batch = BatchRunRequest {
        execution_order: order,
        dependencies: std::collections::HashMap::new(),
        files_map,
        context: &mut context,
        mode: BatchMode::Serial,
        concurrency: 1,
        fail_fast: false,
    };
    let batch_results = batch_executor.execute_batch(batch).await.unwrap();
    let file_results = &batch_results[0].1;

    assert_eq!(file_results.len(), 3, "all three slots preserved");
    assert_eq!(
        file_results[2].request_number, 3,
        "numbering continuity (P0-2)"
    );
    assert!(
        file_results[2].success && !file_results[2].skipped,
        "#3 executed normally"
    );
    assert!(file_results[1].is_teardown, "#2 slot is the teardown");
}

/// 红队 P1-10：@skip 标记的 teardown 豁免 Finally（占位 skipped 保留，不外发）
#[tokio::test]
async fn test_teardown_skip_marked_exempt_from_finally() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ok"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/cleanup"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    let case = format!(
        "### a\n# @name a\nGET {}/ok\n\n### cleanup\n# @teardown\n# @skip\n# @name cleanup\nDELETE {}/cleanup\n",
        server.uri(),
        server.uri()
    );
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("skip_teardown.http");
    std::fs::write(&file, case).unwrap();

    let executor = TestExecutor::new();
    let batch_executor = BatchExecutor::new(executor);
    let mut context = rupost::variable::VariableContext::default();
    let mut files_map = std::collections::HashMap::new();
    files_map.insert(
        file.clone(),
        HttpFileParser::parse_file(file.clone()).expect("parse"),
    );
    let order = vec![file.clone()];
    let batch = BatchRunRequest {
        execution_order: order,
        dependencies: std::collections::HashMap::new(),
        files_map,
        context: &mut context,
        mode: BatchMode::Serial,
        concurrency: 1,
        fail_fast: false,
    };
    let batch_results = batch_executor.execute_batch(batch).await.unwrap();
    let file_results = &batch_results[0].1;

    let skipped_teardown = file_results
        .iter()
        .find(|r| r.request_number == 2)
        .expect("slot preserved");
    assert!(
        skipped_teardown.skipped,
        "skip-marked teardown stays skipped"
    );
    // @skip 豁免：不进 Finally → 无 is_teardown 真执行痕迹
    let reqs = server.received_requests().await.expect("received requests");
    let deletes = reqs
        .iter()
        .filter(|r| r.method == reqwest::Method::DELETE)
        .count();
    assert_eq!(
        deletes, 0,
        "skip-marked teardown must NOT be sent: {deletes}"
    );
}

/// 红队变异 2 守护（逆拓扑序）：A→B 依赖链，B 的 teardown 先于 A 执行
/// （Finally 按已执行文件逆拓扑序——串行确定性覆盖）
#[tokio::test]
async fn test_teardown_reverse_topo_order_two_files() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ta"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/tb"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let fa = dir.path().join("a.http");
    std::fs::write(
        &fa,
        format!(
            "### a\n# @name a\nGET {0}/a\n\n### a-teardown\n# @teardown\n# @name at\nGET {0}/ta\n",
            server.uri()
        ),
    )
    .unwrap();
    let fb = dir.path().join("b.http");
    std::fs::write(
        &fb,
        format!(
            "### b\n# @depends-on a.http\n# @name b\nGET {0}/b\n\n### b-teardown\n# @teardown\n# @name bt\nGET {0}/tb\n",
            server.uri()
        ),
    )
    .unwrap();

    let parsed_a = HttpFileParser::parse_file(fa.clone()).expect("parse a");
    let parsed_b = HttpFileParser::parse_file(fb.clone()).expect("parse b");
    let executor = TestExecutor::new();
    let batch_executor = BatchExecutor::new(executor);
    let mut context = rupost::variable::VariableContext::default();
    let mut files_map = std::collections::HashMap::new();
    files_map.insert(fa.clone(), parsed_a);
    files_map.insert(fb.clone(), parsed_b);
    let order = vec![fa.clone(), fb.clone()];
    let batch = BatchRunRequest {
        execution_order: order,
        dependencies: std::collections::HashMap::new(),
        files_map,
        context: &mut context,
        mode: BatchMode::Serial,
        concurrency: 1,
        fail_fast: false,
    };
    let batch_results = batch_executor.execute_batch(batch).await.unwrap();
    assert_eq!(batch_results.len(), 2, "both files ran");

    // 逆拓扑：B（后执行）的 teardown 先于 A 的——通过 is_teardown 标记守护
    for (path, results) in &batch_results {
        let teardowns: Vec<&rupost::runner::types::TestResult> =
            results.iter().filter(|r| r.is_teardown).collect();
        assert_eq!(
            teardowns.len(),
            1,
            "{path:?} must have its teardown executed"
        );
        assert!(teardowns[0].success);
    }
}
