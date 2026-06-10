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
    rupost::runner::reporter::TestReporter::report_json(&results, &mut out).unwrap();
    let json_str = String::from_utf8(out).unwrap();
    let parsed_json: serde_json::Value = serde_json::from_str(&json_str).unwrap();

    assert!(parsed_json.is_array());
    let arr = parsed_json.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(
        arr[0]["file_path"].as_str().unwrap(),
        file_a.to_str().unwrap()
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
