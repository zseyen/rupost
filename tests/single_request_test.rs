//! DBG-1 单请求重跑选择器集成测试（DX-Debug PRD US-R1）。
//!
//! 锁定契约：`rupost t case.http#3` / `--only <n|@name>` 仅执行选中请求
//! （其余走既有 @skip 语义不发包）、`request_number` 保留文件内原序号、
//! 越界/未命中/目录/互斥四类可读错误、退出码反映选中请求本身。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn rupost_bin() -> &'static str {
    env!("CARGO_BIN_EXE_rupost")
}

fn run(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(rupost_bin())
        .current_dir(dir)
        .env("RUPOST_HISTORY_DIR", dir.join("history_dir"))
        .args(args)
        .output()
        .expect("Failed to execute rupost binary")
}

/// 三请求用例：first/second/third 三个端点，body 分别为 one/two/three。
/// 返回用例路径与 MockServer（调用方持有以保活）。
async fn setup(dir: &Path, third_status: u16) -> (String, MockServer) {
    let server = MockServer::start().await;
    for (endpoint, body) in [("/first", "one"), ("/second", "two")] {
        Mock::given(method("GET"))
            .and(path(endpoint))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/third"))
        .respond_with(ResponseTemplate::new(third_status).set_body_string("three"))
        .mount(&server)
        .await;

    let case: PathBuf = dir.join("case.http");
    fs::write(
        &case,
        format!(
            "###\n# @name first\nGET {}/first\n\n###\n# @name second\nGET {}/second\n\n###\n# @name third\nGET {}/third\n",
            server.uri(),
            server.uri(),
            server.uri()
        ),
    )
    .unwrap();
    (case.to_string_lossy().to_string(), server)
}

fn json_stdout(output: &std::process::Output) -> serde_json::Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim()).expect("stdout should be a pure JSON report")
}

#[tokio::test]
async fn test_ordinal_fragment_runs_only_third_with_original_number() {
    let dir = tempdir().unwrap();
    let (case, _server) = setup(dir.path(), 200).await;

    let out = run(dir.path(), &["t", &format!("{case}#3"), "--report", "json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let report = json_stdout(&out);
    let results = report[0]["results"].as_array().unwrap();
    assert_eq!(results.len(), 3, "skipped 结果保留在报告中");
    assert_eq!(results[0]["skipped"], serde_json::json!(true));
    assert_eq!(results[1]["skipped"], serde_json::json!(true));
    assert_eq!(results[2]["skipped"], serde_json::json!(false));
    // request_number 保留文件内原序号（与批量报告/E2 triage 标识可关联）
    assert_eq!(results[2]["request_number"], serde_json::json!(3));
    assert_eq!(results[2]["name"], serde_json::json!("third"));
    assert_eq!(results[2]["status"], serde_json::json!(200));
}

#[tokio::test]
async fn test_name_flag_runs_second() {
    let dir = tempdir().unwrap();
    let (case, _server) = setup(dir.path(), 200).await;

    let out = run(
        dir.path(),
        &["t", &case, "--only", "@second", "--report", "json"],
    );
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let report = json_stdout(&out);
    let results = report[0]["results"].as_array().unwrap();
    assert_eq!(results[1]["request_number"], serde_json::json!(2));
    assert_eq!(results[1]["skipped"], serde_json::json!(false));
    assert_eq!(results[1]["name"], serde_json::json!("second"));
    assert_eq!(results[0]["skipped"], serde_json::json!(true));
    assert_eq!(results[2]["skipped"], serde_json::json!(true));
}

#[tokio::test]
async fn test_out_of_range_friendly_error() {
    let dir = tempdir().unwrap();
    let (case, _server) = setup(dir.path(), 200).await;

    let out = run(dir.path(), &["t", &format!("{case}#9")]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("合法范围"), "stderr: {stderr}");
    assert!(stderr.contains("1–3"), "stderr: {stderr}");
}

#[tokio::test]
async fn test_name_not_found_lists_available_names() {
    let dir = tempdir().unwrap();
    let (case, _server) = setup(dir.path(), 200).await;

    let out = run(dir.path(), &["t", &case, "--only", "nosuch"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("未找到名为"), "stderr: {stderr}");
    // 列出可用的 @name（first/second/third）
    assert!(
        stderr.contains("first") && stderr.contains("third"),
        "stderr: {stderr}"
    );
}

#[tokio::test]
async fn test_selector_rejects_directory() {
    let dir = tempdir().unwrap();
    let (case, _server) = setup(dir.path(), 200).await;
    let case_dir = dir.path().join("cases");
    fs::create_dir(&case_dir).unwrap();
    fs::write(
        case_dir.join("inner.http"),
        fs::read_to_string(&case).unwrap(),
    )
    .unwrap();

    let out = run(
        dir.path(),
        &["t", case_dir.to_string_lossy().as_ref(), "--only", "1"],
    );
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("不支持目录"), "stderr: {stderr}");
}

#[tokio::test]
async fn test_fragment_and_flag_are_mutually_exclusive() {
    let dir = tempdir().unwrap();
    let (case, _server) = setup(dir.path(), 200).await;

    let out = run(dir.path(), &["t", &case, "#3", "--only", "3"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("互斥"), "stderr: {stderr}");
}

#[tokio::test]
async fn test_exit_code_reflects_selected_request_failure() {
    let dir = tempdir().unwrap();
    // 第三条（被选中的）返回 500：其余跳过不应掩盖失败
    let (case, _server) = setup(dir.path(), 500).await;

    let out = run(dir.path(), &["t", &format!("{case}#3")]);
    assert!(!out.status.success(), "选中请求失败时必须以非零码退出");
}

#[tokio::test]
async fn test_duplicate_name_executes_first_with_warning() {
    let dir = tempdir().unwrap();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;

    let case = dir.path().join("dup.http");
    fs::write(
        &case,
        format!(
            "###\n# @name dup\nGET {}/a\n\n###\n# @name dup\nGET {}/b\n",
            server.uri(),
            server.uri()
        ),
    )
    .unwrap();

    let out = run(
        dir.path(),
        &["t", case.to_string_lossy().as_ref(), "--only", "dup"],
    );
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("已执行首个"), "stderr: {stderr}");
}
