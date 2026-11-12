// 进程级静音历史落盘
mod common;

use rupost::mock::compiler::MockCompiler;
use rupost::mock::matcher::TrieRouteMatcher;
use rupost::mock::server::axum_adapter::serve_matcher;
use rupost::parser::MarkdownFileParser;
use rupost::report::generate_html_report_from_json;
use rupost::report::{JsonAssertionReport, JsonFileReport, JsonTestReport};
use rupost::runner::{TestSuiteOptions, run_test_suite};
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

#[tokio::test]
async fn test_e2e_run_test_suite_generates_html_report() {
    let temp_dir = TempDir::new().unwrap();
    let http_file = temp_dir.path().join("demo.http");
    let html_report_file = temp_dir.path().join("report.html");

    // 编写一个包含一个跳过请求和一个本地请求的 HTTP 测试文件
    let content = r#"
### 1. 模拟简单本地请求
# @skip
GET http://127.0.0.1:9/skipped

### 2. 本地格式测试
# @assert status == 200
GET http://example.com/api/test
"#;
    std::fs::write(&http_file, content).unwrap();

    let options = TestSuiteOptions {
        paths: vec![http_file.to_string_lossy().to_string()],
        mode: "sequential".to_string(),
        concurrency: 1,
        report: "html".to_string(),
        report_file: Some(html_report_file.to_string_lossy().to_string()),
        explain: false,
        triage: false,
        fail_fast: false,
        env: Some("production".to_string()),
        var: vec![],
        env_file: None,
        verbose: false,
        no_cookies: true,
        cookie_file: None,
        debug: false,
        debug_on_failure: false,
        default_scheme: "http".to_string(),
        save_snapshot: None,
        mask_snapshot: false,
        max_snapshot_body_bytes: None,
        only: None,
        on_failure: None,
        allow_missing_plugins: false,
        strict_plugins: false,
        plugin_timeout_secs: 5,
        plugin_env: vec![],
        allow_project_plugins: false,
    };

    // 执行套件并显式写出报告（报告落盘已上移至表现层门面）
    let suite_outcome = run_test_suite(options).await.unwrap();
    let write_outcome = rupost::report::write_report_files(
        &suite_outcome.batch_results,
        "html",
        Some(html_report_file.to_string_lossy().as_ref()),
        Some("production"),
    )
    .unwrap()
    .expect("report_file specified must produce a write outcome");
    assert!(
        !write_outcome.overwritten,
        "tempdir fresh path must not be flagged as overwritten"
    );

    // 验证 HTML 文件已生成
    assert!(
        html_report_file.exists(),
        "必须成功生成指定的 HTML 报告文件"
    );

    let html_content = std::fs::read_to_string(&html_report_file).unwrap();

    // 验证自包含 HTML 关键标签与品牌规范
    assert!(html_content.contains("<!DOCTYPE html>"));
    assert!(html_content.contains("rupost"));
    assert!(html_content.contains("production"));
    assert!(html_content.contains("通过率"));
    assert!(html_content.contains("总执行耗时"));
    assert!(html_content.contains("demo.http"));
    assert!(html_content.contains("m-get"));

    // 验证内联样式与极简控制脚本（无外部 CDN 依赖）
    assert!(html_content.contains("--bg-primary: hsl(220, 16%, 10%);"));
    assert!(html_content.contains("toggleAllDetails"));
    assert!(html_content.contains("filterRequests"));
    assert!(!html_content.contains("https://cdn.jsdelivr.net"));
}

#[tokio::test]
async fn test_e2e_html_report_with_mock_and_waterfall() {
    let temp_dir = TempDir::new().unwrap();
    let spec_path = PathBuf::from("examples/scenarios/scenario_c_ai_collaboration/ai_spec.md");
    let parsed_spec = MarkdownFileParser::parse_file(&spec_path).unwrap();

    // 启动本地 SSE Mock 实例
    let routes = MockCompiler::compile(&parsed_spec);
    let mut matcher = TrieRouteMatcher::new();
    for r in routes {
        matcher.add_route(&r.method, &r.path, r.variants);
    }
    let matcher = Arc::new(matcher);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock server port");
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        if let Err(e) = serve_matcher(listener, matcher, false).await {
            eprintln!("mock server terminated: {e}");
        }
    });

    // 编写带诊断和断言的测试套件
    let http_file = temp_dir.path().join("ai_chat.http");
    let html_output = temp_dir.path().join("ai_report.html");

    let http_content = format!(
        r#"
### AI 流式补全测试
POST http://127.0.0.1:{}/api/v1/chat/completions
Content-Type: application/json
Accept: text/event-stream
@sse
@sse_timeout 5s
@diagnose

{{
  "model": "gpt-4o",
  "messages": [{{"role": "user", "content": "hi"}}]
}}

@assert status == 200
@assert stream.llm.content contains "Hello from Rupost!"
"#,
        port
    );
    std::fs::write(&http_file, http_content).unwrap();

    let options = TestSuiteOptions {
        paths: vec![http_file.to_string_lossy().to_string()],
        mode: "sequential".to_string(),
        concurrency: 1,
        report: "terminal".to_string(),
        report_file: Some(html_output.to_string_lossy().to_string()),
        explain: false,
        triage: false,
        fail_fast: false,
        env: Some("staging".to_string()),
        var: vec![],
        env_file: None,
        verbose: false,
        no_cookies: true,
        cookie_file: None,
        debug: false,
        debug_on_failure: false,
        default_scheme: "http".to_string(),
        save_snapshot: None,
        mask_snapshot: false,
        max_snapshot_body_bytes: None,
        only: None,
        on_failure: None,
        allow_missing_plugins: false,
        strict_plugins: false,
        plugin_timeout_secs: 5,
        plugin_env: vec![],
        allow_project_plugins: false,
    };

    let outcome = run_test_suite(options).await.unwrap();
    assert!(!outcome.has_failure, "测试套件应当全部 PASS");

    // 报告落盘已上移至表现层门面：显式写出（review 假守护点 :59/:156 的真适配）
    let html_output_path = html_output.to_string_lossy().to_string();
    rupost::report::write_report_files(
        &outcome.batch_results,
        "html",
        Some(html_output_path.as_str()),
        None,
    )
    .unwrap()
    .expect("report_file specified must produce a write outcome");

    assert!(html_output.exists(), "HTML 报告应写入文件");
    let html = std::fs::read_to_string(&html_output).unwrap();

    // 验证时延瀑布图生成
    assert!(html.contains("网络时延瀑布图"));
    assert!(html.contains("wf-dns") || html.contains("wf-tcp") || html.contains("wf-ttfb"));
    // 验证断言比对证据
    assert!(html.contains("Hello from Rupost!"));
    assert!(html.contains("assert-pass"));
}

#[test]
fn test_offline_json_report_to_html_generation() {
    let assert_item = JsonAssertionReport {
        raw: "status == 200".to_string(),
        passed: true,
        message: None,
        expected: Some("200".to_string()),
        actual: Some("200".to_string()),
        stream_event_index: None,
    };

    let test_item = JsonTestReport {
        request_number: 1,
        name: Some("query_user".to_string()),
        method: "GET".to_string(),
        url: "https://api.example.com/users/42".to_string(),
        status: Some(200),
        duration_ms: 85,
        success: true,
        error: None,
        skipped: false,
        assertions: vec![assert_item],
        warnings: vec![],
        diagnose_report: None,
        response_body_snippet: None,
    };

    let file_report = JsonFileReport {
        file_path: "tests/users.http".to_string(),
        results: vec![test_item],
    };

    // 从已存在的 JSON 报告结构中生成 HTML
    let html =
        generate_html_report_from_json(&[file_report], Some("Offline Audit"), Some("ci-env"))
            .expect("Should generate HTML cleanly");

    assert!(html.contains("Offline Audit"));
    assert!(html.contains("ci-env"));
    assert!(html.contains("https://api.example.com/users/42"));
    assert!(html.contains("query_user"));
    assert!(html.contains("status == 200"));
}

/// 走查缺口 #2：HTML 在线直渲路径的失败 body_snippet 必须与 JSON 出口同源脱敏
/// （redact_and_truncate_body）——HTML 工件常随 CI 归档共享，不得弱于机器可读出口。
#[tokio::test]
async fn test_e2e_html_report_masks_secrets_in_failed_body_snippet() {
    let temp_dir = TempDir::new().unwrap();

    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/whoami"))
        .respond_with(
            wiremock::ResponseTemplate::new(500).set_body_json(serde_json::json!({
                "error": "internal",
                "token": "super-secret-token-value",
                "api_key": "sk-live-abcdef123456"
            })),
        )
        .mount(&server)
        .await;

    let http_file = temp_dir.path().join("leak.http");
    std::fs::write(
        &http_file,
        format!(
            "###\n# @assert status == 200\nGET {}/whoami?api_key=sk-query-secret-999\n",
            server.uri()
        ),
    )
    .unwrap();

    let html_report_file = temp_dir.path().join("report.html");
    let options = TestSuiteOptions {
        paths: vec![http_file.to_string_lossy().to_string()],
        report: "html".to_string(),
        report_file: Some(html_report_file.to_string_lossy().to_string()),
        no_cookies: true,
        ..TestSuiteOptions::default()
    };
    let suite_outcome = run_test_suite(options).await.unwrap();
    assert!(suite_outcome.has_failure, "断言失败场景是前提");
    rupost::report::write_report_files(
        &suite_outcome.batch_results,
        "html",
        Some(html_report_file.to_string_lossy().as_ref()),
        None,
    )
    .unwrap()
    .expect("report_file specified must produce a write outcome");

    let html_content = std::fs::read_to_string(&html_report_file).unwrap();
    assert!(
        html_content.contains("[REDACTED]"),
        "失败片段中的敏感键必须被掩码"
    );
    assert!(
        !html_content.contains("super-secret-token-value"),
        "敏感值不得原样进入 HTML 工件"
    );
    assert!(
        !html_content.contains("sk-live-abcdef123456"),
        "敏感值不得原样进入 HTML 工件"
    );
    // 审查扩展：顶层 URL 的 query 凭据与 JSON 出口同口径掩码（redact_url）
    assert!(
        !html_content.contains("sk-query-secret-999"),
        "URL query 凭据不得原样进入 HTML 工件"
    );
    assert!(
        html_content.contains("api_key="),
        "query 键名保留（掩码只针对值）"
    );
}
