//! `rupost import curl` 集成测试：devtools 形态 curl → 可执行 `.http` 请求块。

use std::process::Command;

fn rupost_bin() -> &'static str {
    env!("CARGO_BIN_EXE_rupost")
}

fn devtools_curl() -> String {
    "curl -X POST 'http://127.0.0.1:9007/v1/users/1' \
     -H 'Content-Type: application/json' \
     -H 'Authorization: Bearer eyJreal' \
     --data-raw '{\"name\":\"rex\"}'"
        .to_string()
}

#[test]
fn test_import_curl_stdout_contains_request_block() {
    let output = Command::new(rupost_bin())
        .args(["import", "curl", &devtools_curl()])
        .output()
        .expect("failed to run rupost import curl");

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("### POST /v1/users/1"), "stdout: {stdout}");
    assert!(
        stdout.contains("# @name post_v1_users_1"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("authorization: Bearer {{authorization}}"),
        "凭据应模板化: {stdout}"
    );
    assert!(!stdout.contains("eyJreal"), "凭据不得明文落盘: {stdout}");
    assert!(
        stdout.contains("# @assert status == 200"),
        "stdout: {stdout}"
    );
}

#[test]
fn test_import_curl_output_file_create_then_reject_overwrite() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("imported.http");

    let ok = Command::new(rupost_bin())
        .args([
            "import",
            "curl",
            &devtools_curl(),
            "-o",
            out.to_str().expect("utf8"),
        ])
        .output()
        .expect("failed to run import with -o");
    assert_eq!(ok.status.code(), Some(0));
    assert!(out.exists());
    let first = std::fs::read_to_string(&out).unwrap();
    assert!(first.contains("### POST /v1/users/1"));

    // 防覆盖：再次导入同 -o 必须失败且原文件内容不变
    let dup = Command::new(rupost_bin())
        .args([
            "import",
            "curl",
            &devtools_curl(),
            "-o",
            out.to_str().expect("utf8"),
        ])
        .output()
        .expect("failed to run second import");
    assert_ne!(dup.status.code(), Some(0), "重复导入应失败");
    let second = std::fs::read_to_string(&out).unwrap();
    assert_eq!(first, second, "防覆盖：原文件内容不得变化");
}

#[test]
fn test_import_curl_unsupported_flag_rejected() {
    let out = Command::new(rupost_bin())
        .args(["import", "curl", "curl -F file=@x https://a.b"])
        .output()
        .expect("failed to run import");
    assert_ne!(out.status.code(), Some(0));
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.contains("-F"),
        "错误应指明不支持的参数: {combined}"
    );
}

#[test]
fn test_import_curl_empty_data_is_valid_empty_body() {
    // curl 惯用法：`-d ''` 强制 POST 且空体——不得误判为"缺少值"
    let out = Command::new(rupost_bin())
        .args(["import", "curl", "curl -d '' -X POST https://a.b/api"])
        .output()
        .expect("failed to run import");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.contains("POST"),
        "empty -d must force POST: {combined}"
    );
}

#[test]
fn test_import_curl_data_file_reference_rejected() {
    // @file 读取为 MVP 限制之外：显式报错，而非静默导入字面量
    let out = Command::new(rupost_bin())
        .args(["import", "curl", "curl -d @payload.json https://a.b/api"])
        .output()
        .expect("failed to run import");
    assert_ne!(out.status.code(), Some(0));
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.contains("@file") || combined.contains("MVP"),
        "must surface the MVP limitation: {combined}"
    );
}
