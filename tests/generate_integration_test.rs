use chrono::Utc;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use rupost::generator::http::HttpGenerator;
use rupost::history::model::{HistoryEntry, RequestSnapshot, ResponseMeta};
use rupost::parser::HttpFileParser;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn create_test_entry(
    id: &str,
    method_str: &str,
    url: &str,
    headers: Vec<(&str, &str)>,
    body: Option<&str>,
    timeout: Option<Duration>,
    status: u16,
) -> HistoryEntry {
    let mut header_map = HeaderMap::new();
    for (k, v) in headers {
        if let (Ok(key), Ok(val)) = (k.parse::<HeaderName>(), v.parse::<HeaderValue>()) {
            header_map.insert(key, val);
        }
    }

    HistoryEntry {
        id: id.to_string(),
        timestamp: Utc::now(),
        duration_ms: 120,
        request: RequestSnapshot {
            timeout,
            method: method_str.to_string(),
            url: url.to_string(),
            headers: header_map,
            body: body.map(str::to_string),
        },
        source: None,
        response: ResponseMeta {
            status,
            headers: HeaderMap::new(),
            body: Some("{\"ok\":true}".to_string()),
        },
    }
}

fn rupost_bin() -> &'static str {
    env!("CARGO_BIN_EXE_rupost")
}

fn run_rupost_cmd(history_dir: &Path, working_dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(rupost_bin())
        .current_dir(working_dir)
        .env("RUPOST_HISTORY_DIR", history_dir)
        .args(args)
        .output()
        .expect("Failed to execute rupost binary")
}

#[test]
fn test_generate_formats_single_entry_roundtrip_parsable() {
    let entry = create_test_entry(
        "abc12345extra",
        "POST",
        "http://api.example.com/users",
        vec![
            ("Content-Type", "application/json"),
            ("User-Agent", "rupost/1.0.0"),
            ("Authorization", "Bearer my_secret_token"),
        ],
        Some("{\"name\":\"alice\",\"roles\":[\"a\",\"b\"]}"),
        Some(Duration::from_millis(500)),
        201,
    );

    let generated = HttpGenerator::generate(&[entry]).expect("generate failed");

    // 1. 验证生成的文本内容
    assert!(generated.contains("### Request abc12345"));
    assert!(generated.contains("# @timeout 500ms"));
    assert!(generated.contains("POST http://api.example.com/users"));
    assert!(
        generated
            .to_lowercase()
            .contains("content-type: application/json")
    );
    // 敏感凭据必须脱敏（复用 exporter 的 redact 语义），原始 token 严禁出现
    let lowered = generated.to_lowercase();
    assert!(
        lowered.contains("authorization: bearer [redacted]"),
        "Authorization should be redacted, got: {generated}"
    );
    assert!(!generated.contains("my_secret_token"));
    assert!(generated.contains("# @assert status == 201"));

    // 2. 验证 User-Agent 等自计算 Header 已被剥离
    assert!(!generated.to_lowercase().contains("user-agent:"));

    // 3. 验证 JSON 格式被美化格式化
    assert!(generated.contains("\"name\": \"alice\""));
    assert!(generated.contains("[\n    \"a\",\n    \"b\"\n  ]"));

    // 4. 往返解析验证：生成出的 .http 文本必须能被标准 HttpFileParser 准确无误解析
    let parsed_file =
        HttpFileParser::parse_content(&generated).expect("parse generated http failed");
    assert_eq!(parsed_file.requests.len(), 1);

    let req = &parsed_file.requests[0];
    assert_eq!(req.method_or_default(), "POST");
    assert_eq!(req.url, "http://api.example.com/users");
    assert_eq!(req.metadata.timeout, Some(Duration::from_millis(500)));

    let has_status_assert = req
        .metadata
        .assertions
        .iter()
        .any(|a| a.to_string().contains("status == 201"));
    assert!(has_status_assert, "Missing status == 201 assertion");
}

#[test]
fn test_generate_redacts_sensitive_credentials() {
    let entry = create_test_entry(
        "redact00001",
        "GET",
        "https://api.example.com/users?api_key=secret999&page=2",
        vec![
            ("Content-Type", "application/json"),
            ("X-Api-Key", "sk-1234567890"),
            ("Cookie", "session_id=abc123xyz"),
        ],
        None,
        None,
        200,
    );

    let generated = HttpGenerator::generate(&[entry]).expect("generate failed");

    // Header 凭据脱敏，保留认证 scheme 前缀
    let lowered = generated.to_lowercase();
    assert!(lowered.contains("x-api-key: [redacted]"));
    assert!(lowered.contains("cookie: [redacted]"));
    assert!(!generated.contains("sk-1234567890"));
    assert!(!generated.contains("session_id=abc123xyz"));

    // URL Query 中的敏感键值脱敏，普通参数保留
    assert!(!generated.contains("secret999"));
    assert!(lowered.contains("api_key="));
    assert!(generated.contains("page=2"));

    // 非敏感 Header 原样保留
    assert!(
        lowered.contains("content-type: application/json"),
        "non-sensitive header must survive redaction"
    );

    // 生成文本仍可被解析器往返
    let parsed_file =
        HttpFileParser::parse_content(&generated).expect("parse redacted http failed");
    assert_eq!(parsed_file.requests.len(), 1);
}

#[test]
fn test_generate_multiple_entries_join_separator() {
    let entries = vec![
        create_test_entry(
            "id111111",
            "GET",
            "https://api.example.com/a",
            vec![],
            None,
            None,
            200,
        ),
        create_test_entry(
            "id222222",
            "POST",
            "https://api.example.com/b",
            vec![("Content-Type", "text/plain")],
            Some("hello"),
            None,
            201,
        ),
        create_test_entry(
            "id333333",
            "DELETE",
            "https://api.example.com/c",
            vec![],
            None,
            None,
            204,
        ),
    ];

    let generated = HttpGenerator::generate(&entries).expect("generate multiple entries failed");

    // 验证由三个请求块组成，且能被整体无歧义切分解析
    let parsed_file =
        HttpFileParser::parse_content(&generated).expect("parse multiple entries failed");
    assert_eq!(parsed_file.requests.len(), 3);
    assert_eq!(parsed_file.requests[0].method_or_default(), "GET");
    assert_eq!(parsed_file.requests[1].method_or_default(), "POST");
    assert_eq!(parsed_file.requests[2].method_or_default(), "DELETE");
}

#[tokio::test]
async fn test_generate_cli_from_history_end_to_end() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/orders"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .mount(&mock_server)
        .await;

    let dir = tempdir().unwrap();
    let history_dir = dir.path().join("history_dir");
    fs::create_dir_all(&history_dir).unwrap();

    let initial_http = dir.path().join("case.http");
    fs::write(&initial_http, format!("GET {}/orders\n", mock_server.uri())).unwrap();

    // 步骤 1: 真实执行请求，触发历史记录写入
    let out_test1 = run_rupost_cmd(&history_dir, dir.path(), &["t", "case.http"]);
    assert!(
        out_test1.status.success(),
        "Step 1 rupost t failed: {:?}",
        String::from_utf8_lossy(&out_test1.stderr)
    );

    // 步骤 2: 从历史中生成新的测试文件
    let out_gen = run_rupost_cmd(&history_dir, dir.path(), &["g", "out.http", "--last", "1"]);
    let stdout_gen = String::from_utf8_lossy(&out_gen.stdout);
    assert!(
        out_gen.status.success(),
        "Step 2 rupost g failed: {:?}",
        String::from_utf8_lossy(&out_gen.stderr)
    );
    assert!(stdout_gen.contains("Generated test file: out.http (1 requests)"));

    let out_http_path = dir.path().join("out.http");
    assert!(out_http_path.exists());
    let out_content = fs::read_to_string(&out_http_path).unwrap();
    assert!(out_content.contains("/orders"));
    assert!(out_content.contains("# @assert status == 200"));

    // 步骤 3: 真实执行生成出的测试文件，验证闭环成立
    let out_test2 = run_rupost_cmd(&history_dir, dir.path(), &["t", "out.http"]);
    assert!(
        out_test2.status.success(),
        "Step 3 rupost t generated out.http failed: {:?}",
        String::from_utf8_lossy(&out_test2.stderr)
    );
}

#[test]
fn test_generate_cli_empty_history_friendly_exit() {
    let dir = tempdir().unwrap();
    let empty_history_dir = dir.path().join("empty_history");
    fs::create_dir_all(&empty_history_dir).unwrap();

    let out_http = dir.path().join("out.http");

    // 当历史为空时，rupost g 应当友好输出警告并以退出码 0 退出，绝不创建无效空文件
    let output = run_rupost_cmd(
        &empty_history_dir,
        dir.path(),
        &["g", "out.http", "--last", "3"],
    );
    assert!(output.status.success());
    assert!(
        !out_http.exists(),
        "out.http should not be created on empty history"
    );
}
