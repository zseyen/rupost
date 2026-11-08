//! 退出码契约 e2e（子进程形态）：`rupost t` 失败 → exit 1，全绿 → exit 0。
//!
//! run_test_suite 签名改造（bool → TestSuiteOutcome）后，失败判定搬入结构体，
//! 编译无法守护其语义（skipped 误算、fail-fast 截断算错均编译不红）——
//! 本文件以子进程 e2e 锁定端到端退出码契约。
mod common;

use std::process::Command;

use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn run_suite(dir: &std::path::Path, history_dir: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rupost"))
        .env("RUPOST_HISTORY_DIR", history_dir)
        .current_dir(dir)
        .args(["t", "suite.http", "--report", "terminal"])
        .output()
        .expect("spawn rupost test subprocess")
}

fn write_suite(dir: &std::path::Path, base: &str) {
    let suite = format!(
        r#"
### ok case
# @assert status == 200
GET {base}/ok

### boom case
# @assert status == 200
GET {base}/boom
"#
    );
    std::fs::write(dir.join("suite.http"), suite).unwrap();
}

/// 失败请求存在 → 退出码 1（DX-Debug/DX 契约：退出码由测试结果决定）
#[tokio::test]
async fn test_suite_failure_exits_nonzero() {
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
    write_suite(dir.path(), &server.uri());
    let history = tempdir().unwrap();

    let out = run_suite(dir.path(), history.path());
    assert_eq!(
        out.status.code(),
        Some(1),
        "suite with failed request must exit 1, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// 全部通过 → 退出码 0
#[tokio::test]
async fn test_suite_all_pass_exits_zero() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let dir = tempdir().unwrap();
    write_suite(dir.path(), &server.uri());
    let history = tempdir().unwrap();

    let out = run_suite(dir.path(), history.path());
    assert_eq!(
        out.status.code(),
        Some(0),
        "all-pass suite must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `--report html` 缺省 --report-file：默认写 CWD 下 rupost-report.html，
/// 且无条件打印"已生成至"提示（进程级 CWD 隔离必须走子进程，禁止 set_current_dir）
#[tokio::test]
async fn test_report_html_default_path_written_in_cwd() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let dir = tempdir().unwrap();
    write_suite(dir.path(), &server.uri());
    let history = tempdir().unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_rupost"))
        .env("RUPOST_HISTORY_DIR", history.path())
        .current_dir(dir.path())
        .args(["t", "suite.http", "--report", "html"])
        .output()
        .expect("spawn rupost test subprocess");

    assert!(out.status.success(), "all-pass suite must exit 0");
    let default_report = dir.path().join("rupost-report.html");
    assert!(
        default_report.exists(),
        "default html report must be written to CWD"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("报告已生成至"),
        "html default path must print generation notice: {stdout}"
    );
}

/// 空扫描（目录内无 .http/.md）不产生任何报告文件——恢复迁移前语义并锁定（审查 P1）
#[tokio::test]
async fn test_empty_scan_writes_no_report_file() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("readme.txt"), "not a test file").unwrap();
    let history = tempdir().unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_rupost"))
        .env("RUPOST_HISTORY_DIR", history.path())
        .current_dir(dir.path())
        .args(["t", ".", "--report-file", "report.json"])
        .output()
        .expect("spawn rupost test subprocess");

    assert!(
        out.status.success(),
        "empty scan must exit 0, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !dir.path().join("report.json").exists(),
        "empty scan must not write a report file"
    );
}
