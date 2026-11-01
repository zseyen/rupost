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
