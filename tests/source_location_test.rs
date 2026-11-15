//! WP-P2a 守护：失败请求可定位到文档行（终端 `path:line` + JSON `source_line`）。
mod common;

use std::process::Command;
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn run(dir: &std::path::Path, history: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rupost"))
        .env("RUPOST_HISTORY_DIR", history)
        .current_dir(dir)
        .args(args)
        .output()
        .expect("spawn rupost")
}

/// 反假守护：删除 suite_presenter 的定位行打印、或 build_json_reports 的
/// source_line 回填，本测试对应段断言必须分别稳定变红。
#[tokio::test]
async fn test_failed_request_reports_source_location() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ok"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/boom"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let dir = tempdir().unwrap();
    let history = tempdir().unwrap();
    // 行号台账：1 标题 / 2 空 / 3 围栏 / 4 GET ok / 5 空 / 6 assert / 7 围栏
    //           8 空 / 9 围栏 / 10 GET boom / 11 空 / 12 assert / 13 围栏
    let doc = format!(
        "标题\n\n```http\nGET {}/ok\n\n# @assert status == 200\n```\n\n```http\nGET {}/boom\n\n# @assert status == 200\n```\n",
        server.uri(),
        server.uri()
    );
    std::fs::write(dir.path().join("doc.md"), &doc).unwrap();

    // JSON 报告：source_line 字段（json 模式 stdout 为纯 JSON 数组）
    let out = run(
        dir.path(),
        history.path(),
        &["t", "doc.md", "--report", "json"],
    );
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let report: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("json 报告应为纯 JSON 数组");
    let boom = &report[0]["results"][1];
    assert_eq!(boom["request_number"], 2);
    assert_eq!(boom["source_line"], 10, "boom 块方法行应为第 10 行: {boom}");

    // 终端报告：失败块输出 `doc.md:10` 稳定定位
    let out = run(dir.path(), history.path(), &["t", "doc.md"]);
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("doc.md:10"),
        "终端必须输出稳定定位: {stdout}"
    );
}

/// .http 路径同契约（### 分隔块；方法行 = 块内首个请求行）
#[tokio::test]
async fn test_source_location_http_path() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/bad"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let dir = tempdir().unwrap();
    let history = tempdir().unwrap();
    // 行号台账：1 ### / 2 assert / 3 GET
    std::fs::write(
        dir.path().join("a.http"),
        format!(
            "### bad\n# @assert status == 200\nGET {}/bad\n",
            server.uri()
        ),
    )
    .unwrap();

    let out = run(
        dir.path(),
        history.path(),
        &["t", "a.http", "--report", "json"],
    );
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let report: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(
        report[0]["results"][0]["source_line"], 3,
        "a.http 方法行应为第 3 行: {}",
        report[0]["results"][0]
    );
}
