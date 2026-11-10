//! DBG-2 / R2：`--debug-on-failure` 失败自动诊断守护测试。
//!
//! 锁定契约（测试方案 §5.9 待补项）：
//! 1. 断言失败（活服务器）→ 自动输出网络诊断瀑布（"RuPost Network Diagnostics" + "DNS Lookup"）；
//! 2. 请求成功 → 零诊断噪音；
//! 3. 连接拒绝（不可达端口）→ 失败退出、零 panic；
//!    （2026-11-10 走查缺口 #5 已闭合：连接层失败现经 `diagnose_url_lenient`
//!    产出带 `connection_error` 阶段分类的部分诊断报告，见测试 ⑤）
//! 4. `--report json` 模式下失败结果的 `diagnose_report` 字段非空（机器可读契约）；
//! 5. 连接拒绝的部分诊断报告：`connection_error` 落在 TCP 阶段、resolved_ips
//!    非空（DNS 成功）而 tls/cert/http_status 为空——DNS/TCP 拆分可观测。

use std::fs;
use std::path::Path;
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

/// 单请求用例：GET {base}/broken，断言 status == 200（必失败）
fn write_failing_case(dir: &Path, base: &str) -> String {
    let case = dir.join("fail.http");
    fs::write(
        &case,
        format!("###\n# @assert status == 200\nGET {base}/broken\n"),
    )
    .unwrap();
    case.to_string_lossy().to_string()
}

fn closed_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

#[tokio::test]
async fn test_debug_on_failure_prints_waterfall_on_assertion_failure() {
    let dir = tempdir().unwrap();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/broken"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&server)
        .await;
    let case = write_failing_case(dir.path(), &server.uri());

    let out = run(dir.path(), &["t", case.as_str(), "--debug-on-failure"]);
    assert!(!out.status.success(), "断言失败必须以非零码退出");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("RuPost Network Diagnostics"),
        "缺诊断瀑布标题, stdout: {stdout}"
    );
    assert!(
        stdout.contains("DNS Lookup"),
        "缺 DNS 拆分段, stdout: {stdout}"
    );
}

#[tokio::test]
async fn test_debug_on_failure_is_silent_on_success() {
    let dir = tempdir().unwrap();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ok"))
        .respond_with(ResponseTemplate::new(200).set_body_string("fine"))
        .mount(&server)
        .await;
    let case = dir.path().join("ok.http");
    fs::write(
        &case,
        format!("###\n# @assert status == 200\nGET {}/ok\n", server.uri()),
    )
    .unwrap();

    let out = run(
        dir.path(),
        &["t", case.to_string_lossy().as_ref(), "--debug-on-failure"],
    );
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("Network Diagnostics"),
        "成功时不得输出诊断, stdout: {stdout}"
    );
    assert!(!stdout.contains("DNS Lookup"), "stdout: {stdout}");
}

#[tokio::test]
async fn test_connection_refused_reports_failure_without_panic() {
    let dir = tempdir().unwrap();
    let port = closed_port();
    let case = dir.path().join("refused.http");
    fs::write(&case, format!("GET http://127.0.0.1:{port}/broken\n")).unwrap();

    let out = run(
        dir.path(),
        &["t", case.to_string_lossy().as_ref(), "--debug-on-failure"],
    );
    assert!(!out.status.success(), "连接失败必须以非零码退出");
    // 零 panic（进程正常退出而非崩溃）；连接层失败的拆分缺口由 Virtual Response 条目承接
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(combined.contains("127.0.0.1"), "combined: {combined}");
}

#[tokio::test]
async fn test_json_report_carries_diagnose_report() {
    let dir = tempdir().unwrap();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/broken"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&server)
        .await;
    let case = write_failing_case(dir.path(), &server.uri());

    let out = run(
        dir.path(),
        &["t", &case, "--debug-on-failure", "--report", "json"],
    );
    assert!(!out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let report: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("stdout 应为纯 JSON 报告");
    let first = &report[0]["results"][0];
    assert_eq!(first["success"], serde_json::json!(false));
    assert!(
        !first["diagnose_report"].is_null(),
        "失败结果应携带诊断报告, json: {report}"
    );
}

/// 走查缺口 #5（D2 连接层拆分）：连接拒绝时 `diagnose_url_lenient` 产出
/// 带 `connection_error` 阶段分类的部分诊断报告——resolved_ips 非空证明
/// DNS 阶段成功、tls/cert/http 字段为空证明失败落在 TCP 阶段。
#[tokio::test]
async fn test_connection_refused_carries_staged_partial_diagnose() {
    let dir = tempdir().unwrap();
    let port = closed_port();
    let case = dir.path().join("refused.http");
    fs::write(&case, format!("GET http://127.0.0.1:{port}/broken\n")).unwrap();

    let out = run(
        dir.path(),
        &[
            "t",
            case.to_string_lossy().as_ref(),
            "--debug-on-failure",
            "--report",
            "json",
        ],
    );
    assert!(!out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let report: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("stdout 应为纯 JSON 报告");
    let diag = &report[0]["results"][0]["diagnose_report"];
    assert!(
        !diag.is_null(),
        "连接层失败也必须携带部分诊断报告, json: {report}"
    );

    // 阶段拆分：DNS 成功（IP 已解析）而 TCP 失败（分类信息非空）
    let conn_err = diag["connection_error"]
        .as_str()
        .expect("connection_error 应携带阶段分类");
    assert!(
        conn_err.starts_with("TCP connection failed"),
        "连接拒绝的分类应落在 TCP 阶段: {conn_err}"
    );
    let ips = diag["resolved_ips"].as_array().expect("resolved_ips 数组");
    assert!(
        !ips.is_empty(),
        "DNS 已成功，127.0.0.1 必须出现在 resolved_ips 中"
    );
    assert_eq!(diag["tls_handshake_duration"], serde_json::Value::Null);
    assert_eq!(diag["cert_info"], serde_json::Value::Null);
    assert_eq!(diag["http_status"], serde_json::Value::Null);
}
