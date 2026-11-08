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
