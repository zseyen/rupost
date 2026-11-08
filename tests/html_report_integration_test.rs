// 进程级静音历史落盘
mod common;

use rupost::mock::compiler::MockCompiler;
use rupost::mock::matcher::TrieRouteMatcher;
use rupost::mock::server::{AxumMockServer, MockServer};
use rupost::parser::MarkdownFileParser;
use rupost::report::generate_html_report_from_json;
use rupost::report::{JsonAssertionReport, JsonFileReport, JsonTestReport};
use rupost::runner::{TestSuiteOptions, run_test_suite};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
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
        only: None,
        on_failure: None,
    };

    // 执行套件
    let _ = run_test_suite(options).await;

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
    let port = 19198;

    let server = AxumMockServer;
    tokio::spawn(async move {
        let _ = server.start(port, matcher).await;
    });
    tokio::time::sleep(Duration::from_millis(80)).await;

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
        only: None,
        on_failure: None,
    };

    let outcome = run_test_suite(options).await.unwrap();
    assert!(!outcome.has_failure, "测试套件应当全部 PASS");

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
