//! 走查缺口 #1：`--no-history` 全局旗标守护测试。
//!
//! CI/共享环境下 `rupost t` 会把完整 headers（含 Authorization/Cookie）与 body
//! 明文写入 `.rupost/history.jsonl`，而报告出口是强制脱敏的——脱敏纪律在同一进程
//! 内不一致。本旗标让运行方显式关停落盘。锁定契约：
//! 1. 默认行为不变：无旗标时 history.jsonl 照常生成（防误伤既有语义）；
//! 2. 子命令后置 `t f.http --no-history`：不落盘；
//! 3. 子命令前置 `--no-history t f.http`：不落盘（AGENTS §5 全局旗标双序契约）；
//! 4. 内联请求 `rupost --no-history GET url`：不落盘；
//! 5. `--report json` 模式下 stdout 仍为纯 JSON（停用提示走 stderr）。

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

async fn start_ok_server() -> (MockServer, String) {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ok"))
        .respond_with(ResponseTemplate::new(200).set_body_string("fine"))
        .mount(&server)
        .await;
    let case = format!("###\n# @assert status == 200\nGET {}/ok\n", server.uri());
    (server, case)
}

fn write_case(dir: &Path, content: &str) -> String {
    let case = dir.join("case.http");
    fs::write(&case, content).unwrap();
    case.to_string_lossy().to_string()
}

fn history_file(dir: &Path) -> std::path::PathBuf {
    dir.join("history_dir").join("history.jsonl")
}

/// 契约 1：默认行为不变——无旗标时照常落盘（防误伤既有语义）
#[tokio::test]
async fn test_default_still_records_history() {
    let dir = tempdir().unwrap();
    let (_server, case) = start_ok_server().await;
    let case = write_case(dir.path(), &case);

    let out = run(dir.path(), &["t", case.as_str()]);
    assert!(out.status.success());
    assert!(
        history_file(dir.path()).exists(),
        "无 --no-history 时必须照常落盘（默认语义不得漂移）"
    );
}

/// 契约 2：子命令后置旗标不落盘 + stdout 保持纯 JSON（提示走 stderr）
#[tokio::test]
async fn test_no_history_post_subcommand_skips_recording() {
    let dir = tempdir().unwrap();
    let (_server, case) = start_ok_server().await;
    let case = write_case(dir.path(), &case);

    let out = run(
        dir.path(),
        &["t", case.as_str(), "--no-history", "--report", "json"],
    );
    assert!(out.status.success());
    assert!(
        !history_file(dir.path()).exists(),
        "--no-history 后置位置必须关停落盘"
    );

    // stdout 纯净性：停用提示不得污染机器可读出口
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("stdout 应为纯 JSON 报告");
    assert_eq!(parsed[0]["results"][0]["success"], serde_json::json!(true));

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("no-history"),
        "停用提示应出现在 stderr: {stderr}"
    );
}

/// 契约 3：子命令前置旗标不落盘（全局旗标双序契约）
#[tokio::test]
async fn test_no_history_pre_subcommand_skips_recording() {
    let dir = tempdir().unwrap();
    let (_server, case) = start_ok_server().await;
    let case = write_case(dir.path(), &case);

    let out = run(dir.path(), &["--no-history", "t", case.as_str()]);
    assert!(out.status.success());
    assert!(
        !history_file(dir.path()).exists(),
        "--no-history 前置位置必须关停落盘"
    );
}

/// 契约 4：内联请求模式同样生效
#[tokio::test]
async fn test_no_history_inline_mode_skips_recording() {
    let dir = tempdir().unwrap();
    let (_server, case) = start_ok_server().await;
    // 从用例文本中抠出 URL 供内联模式使用
    let url = case
        .lines()
        .find(|l| l.starts_with("GET "))
        .unwrap()
        .trim_start_matches("GET ")
        .to_string();

    let out = run(dir.path(), &["--no-history", "GET", url.as_str()]);
    assert!(out.status.success());
    assert!(
        !history_file(dir.path()).exists(),
        "内联请求模式的 --no-history 必须关停落盘"
    );
}
