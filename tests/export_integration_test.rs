use rupost::exporter::run_export;
use rupost::exporter::types::ExportRunOptions;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_export_curl_to_file_and_force_overwrite() {
    let dir = tempdir().unwrap();
    let http_file = dir.path().join("test.http");
    let out_file = dir.path().join("output.sh");

    fs::write(
        &http_file,
        r#"
# @name GetUsers
GET https://api.example.com/users
Accept: application/json

###
# @name CreateUser
POST https://api.example.com/users

{"name": "Bob"}
"#,
    )
    .unwrap();

    let opts = ExportRunOptions {
        target: Some("curl".to_string()),
        paths: vec![http_file.to_string_lossy().to_string()],
        output: Some(out_file.clone()),
        ..Default::default()
    };

    run_export(opts).unwrap();

    assert!(out_file.exists());
    let content = fs::read_to_string(&out_file).unwrap();
    assert!(content.contains("# === [GetUsers]"));
    assert!(content.contains(
        "curl \\\n  -H 'Accept: application/json' \\\n  'https://api.example.com/users'"
    ));
    assert!(content.contains("# === [CreateUser]"));
    assert!(content.contains("-X POST"));
    assert!(content.contains("-H 'Content-Type: application/json'"));
    assert!(content.contains("--data-raw '{\"name\": \"Bob\"}'"));

    // 验证无 --force 时二次写入报错防覆盖保护
    let opts_no_force = ExportRunOptions {
        target: Some("curl".to_string()),
        paths: vec![http_file.to_string_lossy().to_string()],
        output: Some(out_file.clone()),
        force: false,
        ..Default::default()
    };
    assert!(run_export(opts_no_force).is_err());

    // 验证带 force 成功覆盖
    let opts_force = ExportRunOptions {
        target: Some("curl".to_string()),
        paths: vec![http_file.to_string_lossy().to_string()],
        output: Some(out_file.clone()),
        force: true,
        ..Default::default()
    };
    assert!(run_export(opts_force).is_ok());
}

#[test]
fn test_export_filter_by_name() {
    let dir = tempdir().unwrap();
    let http_file = dir.path().join("suite.http");
    let out_file = dir.path().join("single.sh");

    fs::write(
        &http_file,
        r#"
# @name First
GET https://api.example.com/first

###
# @name Second
POST https://api.example.com/second
Content-Type: text/plain

payload
"#,
    )
    .unwrap();

    let opts = ExportRunOptions {
        target: Some("httpie".to_string()),
        paths: vec![http_file.to_string_lossy().to_string()],
        name: Some("Second".to_string()),
        output: Some(out_file.clone()),
        compact: true,
        ..Default::default()
    };

    run_export(opts).unwrap();

    let content = fs::read_to_string(&out_file).unwrap();
    assert!(!content.contains("first"));
    assert!(content.contains(
        "http --raw 'payload' POST 'https://api.example.com/second' 'Content-Type:text/plain'"
    ));
}

#[test]
fn test_export_curl_with_redaction_and_metadata() {
    let dir = tempdir().unwrap();
    let http_file = dir.path().join("secure.http");
    let out_file = dir.path().join("secure_curl.sh");

    fs::write(
        &http_file,
        r#"
# @name SecureReq
# @sensitive x-corp-token
# @no-redact x-api-key-safe
GET https://api.example.com/data?token=super_secret_query&page=1
Authorization: Bearer top_secret_jwt
x-corp-token: raw_corp_credential
x-api-key-safe: open_safe_key
"#,
    )
    .unwrap();

    let opts = ExportRunOptions {
        target: Some("curl".to_string()),
        paths: vec![http_file.to_string_lossy().to_string()],
        output: Some(out_file.clone()),
        redact: true,
        compact: true,
        ..Default::default()
    };

    run_export(opts).unwrap();

    assert!(out_file.exists());
    let content = fs::read_to_string(&out_file).unwrap();

    // 导出的命令应该包含脱敏后的值
    assert!(content.contains("token=%5BREDACTED%5D") || content.contains("token=[REDACTED]"));
    assert!(content.contains("page=1"));
    assert!(content.contains("Authorization: Bearer [REDACTED]"));
    assert!(content.contains("x-corp-token: [REDACTED]"));
    // @no-redact 豁免的 key 保持原值
    assert!(content.contains("x-api-key-safe: open_safe_key"));
    // 原始机密绝不能泄露
    assert!(!content.contains("super_secret_query"));
    assert!(!content.contains("top_secret_jwt"));
    assert!(!content.contains("raw_corp_credential"));
    // 导出的脚本本身纯净，不含审计 footer 注释
    assert!(!content.contains("[i] Redacted"));
}

/// 审查 P1：`--cookie-file` 存在但 JSON 损坏 → stderr 告警 + 导出继续
/// （文件存在即意图；静默会让粘贴方拿到必然 401 的命令且无从归因）
#[test]
fn test_export_corrupt_cookie_file_warns_but_exports() {
    let dir = tempfile::tempdir().unwrap();
    let bad_cookie = dir.path().join("bad_cookies.json");
    std::fs::write(&bad_cookie, "{ not valid json [[[").unwrap();
    let case = dir.path().join("api.http");
    std::fs::write(&case, "###\nGET http://127.0.0.1:1/api\n").unwrap();

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        .args([
            "export",
            "curl",
            case.to_str().unwrap(),
            "--cookie-file",
            bad_cookie.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("解析失败") && stderr.contains("cookie"),
        "损坏 cookie 必须告警: {stderr}"
    );
    // 导出本身照常完成（stdout 有 curl 命令，exit 0）
    assert!(out.status.success(), "告警后导出应继续: {stderr}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("curl"), "导出命令应照常产出: {stdout}");
}
