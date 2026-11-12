//! 黑匣子快照契约与领域模型守护测试（拆分自超长存储文件，保持单文件轻量可维护）。

use rupost::history::model::{
    ExecutionSnapshotFile, MaskingMode, RequestSnapshot, ResponseSnapshot, RunnerFingerprint,
    SNAPSHOT_SCHEMA_VERSION, SnapshotEntry, SnapshotSuite, SocketDiagnostics, TimingWaterfallUs,
};
use rupost::history::storage::{
    read_execution_snapshot, save_batch_snapshot, write_execution_snapshot,
};
use rupost::history::{mask_snapshot_entry, read_snapshot_suite, write_snapshot_suite};
use rupost::http::Response;
use rupost::http::diagnose::{CertInfo, DiagnosticsReport};
use rupost::http::timing::RequestTiming;
use rupost::parser::ParsedRequest;
use rupost::runner::types::TestResult;
use std::path::PathBuf;
use std::time::Duration;

#[test]
fn test_request_snapshot_from_parsed_preserves_multi_value_headers() {
    let mut parsed = ParsedRequest::new(1);
    parsed.method = Some("GET".to_string());
    parsed.url = "https://example.com".to_string();
    parsed.headers = vec![
        ("Cookie".to_string(), "a=1".to_string()),
        ("Cookie".to_string(), "b=2".to_string()),
        ("X-Tag".to_string(), "v1".to_string()),
        ("X-Tag".to_string(), "v2".to_string()),
    ];

    let snap = RequestSnapshot::from_parsed(&parsed);
    let tags: Vec<&str> = snap
        .headers
        .get_all("x-tag")
        .iter()
        .map(|v| v.to_str().unwrap())
        .collect();
    assert_eq!(tags, vec!["v1", "v2"]);
    let cookies: Vec<&str> = snap
        .headers
        .get_all("cookie")
        .iter()
        .map(|v| v.to_str().unwrap())
        .collect();
    assert_eq!(cookies, vec!["a=1", "b=2"]);
}

#[test]
fn test_save_batch_snapshot_excludes_teardown_results() {
    let dir = tempfile::tempdir().unwrap();
    let snapshot_file = dir.path().join("snap.json");

    let resp = Response::new(
        200,
        reqwest::header::HeaderMap::new(),
        "{}".to_string(),
        Duration::from_millis(5),
        Duration::from_millis(5),
        Duration::ZERO,
    )
    .unwrap();

    let mut normal = TestResult::success(
        1,
        Some("normal".to_string()),
        "GET".to_string(),
        "http://x/normal".to_string(),
        resp.clone(),
    );
    normal.request = Some(RequestSnapshot {
        method: "GET".to_string(),
        url: "http://x/normal".to_string(),
        headers: reqwest::header::HeaderMap::new(),
        body: None,
        timeout: None,
    });

    let mut teardown = TestResult::success(
        2,
        Some("cleanup".to_string()),
        "DELETE".to_string(),
        "http://x/cleanup".to_string(),
        resp,
    );
    teardown.is_teardown = true;
    teardown.request = None;

    let batch_results = vec![(PathBuf::from("t.http"), vec![normal, teardown])];
    save_batch_snapshot(
        &batch_results,
        &["t.http".to_string()],
        &snapshot_file,
        false,
        None,
        rupost::history::model::DEFAULT_MAX_SNAPSHOT_BODY_BYTES,
    )
    .unwrap();

    let snap = read_execution_snapshot(&snapshot_file).unwrap();
    assert_eq!(snap.entries.len(), 1);
    assert_eq!(snap.entries[0].request.url, "http://x/normal");
}

#[test]
fn test_execution_snapshot_unknown_field_tolerance() {
    let json = r#"{
        "schema_version": "1.0",
        "generated_at": "2026-11-11T00:00:00Z",
        "environment": null,
        "runner_fingerprint": {"os": "os", "arch": "arch", "rupost_version": "0.2.0"},
        "masking": "none",
        "entries": [],
        "future_field_appended_in_v11": true
    }"#;
    let snap: ExecutionSnapshotFile = serde_json::from_str(json).unwrap();
    assert_eq!(snap.schema_version, "1.0");
    assert!(snap.entries.is_empty());
}

#[test]
fn test_timing_waterfall_from_probes() {
    let timing = RequestTiming {
        dns_lookup: Duration::from_micros(120),
        tcp_connect: Duration::from_micros(3_000),
        ttfb: Duration::from_micros(50_000),
        transfer: Duration::from_micros(1_000),
    };
    let wf = TimingWaterfallUs::from_probes(&timing, None);
    assert_eq!(wf.dns_lookup_us, Some(120));
    assert_eq!(wf.tcp_connect_us, Some(3_000));
    assert_eq!(wf.tls_handshake_us, None);
    assert_eq!(wf.ttfb_us, 50_000);
    assert_eq!(wf.transfer_us, 1_000);
    assert_eq!(wf.total_us, 54_120);
}

#[test]
fn test_socket_diagnostics_from_diagnose() {
    let report = DiagnosticsReport {
        url: "https://api.example.com:8443/v1".to_string(),
        is_https: true,
        is_websocket: false,
        ws_upgrade_success: None,
        resolved_ips: vec!["10.0.0.1".to_string(), "10.0.0.2".to_string()],
        dns_lookup_duration: Duration::from_micros(100),
        tcp_connect_duration: Duration::from_micros(2_000),
        tls_handshake_duration: Some(Duration::from_micros(30_000)),
        cert_info: Some(CertInfo {
            subject: "CN=api.example.com".to_string(),
            issuer: "CN=DigiCert".to_string(),
            sans: vec![],
            validity_not_after: "2027-01-01".to_string(),
            days_remaining: 365,
        }),
        http_status: Some(200),
        http_version: Some("HTTP/2".to_string()),
        ttfb: Some(Duration::from_micros(60_000)),
        total_duration: Duration::from_micros(92_100),
        connection_error: None,
    };
    let socket = SocketDiagnostics::from_diagnose(&report).unwrap();
    assert_eq!(socket.remote_ip, "10.0.0.1");
    assert_eq!(socket.remote_port, 8443);
    assert_eq!(socket.http_version, "HTTP/2");
    assert_eq!(socket.cert_days_remaining, Some(365));
    assert_eq!(socket.cert_issuer.as_deref(), Some("CN=DigiCert"));
}

#[test]
fn test_save_batch_snapshot_roundtrip_fields() {
    let temp_dir = tempfile::tempdir().unwrap();
    let snapshot_file = temp_dir.path().join("snapshot.json");

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("content-type", "application/json".parse().unwrap());

    let req_snap = RequestSnapshot {
        timeout: None,
        method: "POST".to_string(),
        url: "http://example.com/test".to_string(),
        headers: headers.clone(),
        body: Some("{\"key\":\"value\"}".to_string()),
    };

    let response = Response::new(
        201,
        headers,
        "{\"status\":\"ok\"}".to_string(),
        Duration::from_millis(50),
        Duration::from_millis(20),
        Duration::from_millis(30),
    )
    .unwrap();

    let mut test_result = TestResult::success(
        1,
        Some("test_req".to_string()),
        "POST".to_string(),
        "http://example.com/test".to_string(),
        response,
    );
    test_result.request = Some(req_snap);

    let batch_results = vec![(PathBuf::from("my_test.http"), vec![test_result])];
    save_batch_snapshot(
        &batch_results,
        &["my_test.http".to_string()],
        &snapshot_file,
        false,
        Some("staging"),
        rupost::history::model::DEFAULT_MAX_SNAPSHOT_BODY_BYTES,
    )
    .unwrap();

    let snap = read_execution_snapshot(&snapshot_file).unwrap();
    assert_eq!(snap.schema_version, "1.0");
    assert_eq!(snap.environment.as_deref(), Some("staging"));
    assert_eq!(snap.entries.len(), 1);
    let entry = &snap.entries[0];
    assert_eq!(entry.file_path, "my_test.http");
    assert_eq!(entry.status, Some(201));
    assert_eq!(entry.response.as_ref().unwrap().body, "{\"status\":\"ok\"}");
    assert!(entry.plugin_duration_us.is_none());
}

#[cfg(unix)]
#[test]
fn test_write_execution_snapshot_tightens_existing_0644_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("existing.json");
    std::fs::write(&path, "old").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

    let snap = ExecutionSnapshotFile {
        schema_version: SNAPSHOT_SCHEMA_VERSION.to_string(),
        generated_at: chrono::Utc::now(),
        environment: None,
        runner_fingerprint: RunnerFingerprint {
            os: "test".to_string(),
            arch: "test".to_string(),
            rupost_version: "0.0.0".to_string(),
        },
        masking: MaskingMode::None,
        entries: vec![],
    };
    write_execution_snapshot(&snap, &path).unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "覆写已有 0644 文件后权限必须收紧至 0600");
}

/// M6-4（冻结清单缺口 #4）：`--max-snapshot-body-bytes` 旗标接线。
/// 自定义上限必须真实生效：body 截断 + `body_truncated` 标记 + 截断前完整 Body 的
/// SHA-256 验真摘要（03 §10.6）；默认 256KB 路径行为不变（防回归对照）。
#[test]
fn test_save_batch_snapshot_honors_custom_body_limit() {
    let dir = tempfile::tempdir().unwrap();
    let snapshot_file = dir.path().join("snap_limit.json");

    let big_body = "A".repeat(64);
    let resp = Response::new(
        200,
        reqwest::header::HeaderMap::new(),
        big_body.clone(),
        Duration::from_millis(5),
        Duration::from_millis(5),
        Duration::ZERO,
    )
    .unwrap();
    let result = TestResult::success(
        1,
        Some("big".to_string()),
        "GET".to_string(),
        "http://x/big".to_string(),
        resp,
    );
    let batch_results = vec![(PathBuf::from("t.http"), vec![result])];

    // 自定义上限 8 字节 → 截断 + 摘要
    save_batch_snapshot(
        &batch_results,
        &["t.http".to_string()],
        &snapshot_file,
        false,
        None,
        8,
    )
    .unwrap();
    let snap = read_execution_snapshot(&snapshot_file).unwrap();
    let body = snap.entries[0]
        .response
        .as_ref()
        .expect("response recorded");
    assert_eq!(body.body.len(), 8, "body 必须按自定义上限 8 字节截断");
    assert!(body.body_truncated, "截断必须如实标记 body_truncated=true");
    let expected_sha = {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(big_body.as_bytes());
        h.finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    assert_eq!(
        body.body_sha256.as_deref(),
        Some(expected_sha.as_str()),
        "body_sha256 必须为截断前完整 Body 的摘要（离线验真）"
    );

    // 默认上限（256KB）路径：64 字节 body 不截断、无摘要
    let snapshot_file_default = dir.path().join("snap_default.json");
    let resp2 = Response::new(
        200,
        reqwest::header::HeaderMap::new(),
        big_body,
        Duration::from_millis(5),
        Duration::from_millis(5),
        Duration::ZERO,
    )
    .unwrap();
    let result2 = TestResult::success(
        1,
        Some("big".to_string()),
        "GET".to_string(),
        "http://x/big".to_string(),
        resp2,
    );
    let batch2 = vec![(PathBuf::from("t.http"), vec![result2])];
    save_batch_snapshot(
        &batch2,
        &["t.http".to_string()],
        &snapshot_file_default,
        false,
        None,
        rupost::history::model::DEFAULT_MAX_SNAPSHOT_BODY_BYTES,
    )
    .unwrap();
    let snap2 = read_execution_snapshot(&snapshot_file_default).unwrap();
    let body2 = snap2.entries[0].response.as_ref().unwrap();
    assert!(!body2.body_truncated);
    assert!(body2.body_sha256.is_none());
}

#[test]
fn test_mask_snapshot_entry_preserves_and_masks_multi_value_set_cookie_headers() {
    let mut req_headers = reqwest::header::HeaderMap::new();
    req_headers.append("cookie", "session=s1".parse().unwrap());
    req_headers.append("cookie", "csrf=c2".parse().unwrap());
    req_headers.append("x-trace", "trace-1".parse().unwrap());
    req_headers.append("x-trace", "trace-2".parse().unwrap());

    let mut resp_headers = reqwest::header::HeaderMap::new();
    resp_headers.append("set-cookie", "sid=secret_one; HttpOnly".parse().unwrap());
    resp_headers.append("set-cookie", "refresh=secret_two; Path=/".parse().unwrap());
    resp_headers.append("set-cookie", "prefs=secret_three".parse().unwrap());
    resp_headers.append("x-tag", "alpha".parse().unwrap());
    resp_headers.append("x-tag", "beta".parse().unwrap());

    let mut entry = SnapshotEntry::new_success(
        "multi-cookie".to_string(),
        RequestSnapshot {
            method: "POST".to_string(),
            url: "https://api.example.com/login".to_string(),
            headers: req_headers,
            body: None,
            timeout: None,
        },
        ResponseSnapshot {
            status: 200,
            headers: resp_headers,
            body: "ok".to_string(),
        },
    );

    mask_snapshot_entry(&mut entry);

    let cookies: Vec<&str> = entry
        .request
        .headers
        .get_all("cookie")
        .iter()
        .map(|v| v.to_str().unwrap())
        .collect();
    assert_eq!(cookies, vec!["[REDACTED]", "[REDACTED]"]);

    let traces: Vec<&str> = entry
        .request
        .headers
        .get_all("x-trace")
        .iter()
        .map(|v| v.to_str().unwrap())
        .collect();
    assert_eq!(traces, vec!["trace-1", "trace-2"]);

    let resp = entry.response.as_ref().unwrap();
    let set_cookies: Vec<&str> = resp
        .headers
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap())
        .collect();
    assert_eq!(
        set_cookies,
        vec!["[REDACTED]", "[REDACTED]", "[REDACTED]"],
        "多值 Set-Cookie 必须全部保留数量且逐条脱敏"
    );

    let tags: Vec<&str> = resp
        .headers
        .get_all("x-tag")
        .iter()
        .map(|v| v.to_str().unwrap())
        .collect();
    assert_eq!(tags, vec!["alpha", "beta"]);
}

#[cfg(unix)]
#[test]
fn test_snapshot_writes_protect_symlinks_and_enforce_0600_permissions() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let dir = tempfile::TempDir::new().unwrap();

    // 1. write_execution_snapshot: 不跟随符号链接且强约束 0600
    let victim_exec = dir.path().join("victim_exec.txt");
    std::fs::write(&victim_exec, "precious-exec-target").unwrap();
    let link_exec = dir.path().join("exec_snap.json");
    symlink(&victim_exec, &link_exec).unwrap();

    let exec_snap = ExecutionSnapshotFile {
        schema_version: SNAPSHOT_SCHEMA_VERSION.to_string(),
        generated_at: chrono::Utc::now(),
        environment: None,
        runner_fingerprint: RunnerFingerprint {
            os: "test".to_string(),
            arch: "test".to_string(),
            rupost_version: "0.0.0".to_string(),
        },
        masking: MaskingMode::None,
        entries: vec![],
    };
    write_execution_snapshot(&exec_snap, &link_exec).unwrap();

    assert_eq!(
        std::fs::read_to_string(&victim_exec).unwrap(),
        "precious-exec-target",
        "write_execution_snapshot 不得跟随符号链接覆写目标文件"
    );
    assert!(
        !std::fs::symlink_metadata(&link_exec)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let exec_mode = std::fs::metadata(&link_exec).unwrap().permissions().mode() & 0o777;
    assert_eq!(exec_mode, 0o600);
    assert_eq!(
        read_execution_snapshot(&link_exec).unwrap().schema_version,
        "1.0"
    );

    // 2. write_snapshot_suite: 不跟随符号链接且强约束 0600（含新建文件）
    let victim_suite = dir.path().join("victim_suite.txt");
    std::fs::write(&victim_suite, "precious-suite-target").unwrap();
    let link_suite = dir.path().join("legacy_suite.json");
    symlink(&victim_suite, &link_suite).unwrap();

    let suite = SnapshotSuite {
        version: 2,
        timestamp: chrono::Utc::now(),
        source_file: Some("contract_test".to_string()),
        entries: vec![],
    };
    write_snapshot_suite(&suite, &link_suite).unwrap();

    assert_eq!(
        std::fs::read_to_string(&victim_suite).unwrap(),
        "precious-suite-target",
        "write_snapshot_suite 不得跟随符号链接覆写目标文件"
    );
    assert!(
        !std::fs::symlink_metadata(&link_suite)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let suite_mode = std::fs::metadata(&link_suite).unwrap().permissions().mode() & 0o777;
    assert_eq!(suite_mode, 0o600);
    assert_eq!(read_snapshot_suite(&link_suite).unwrap().version, 2);

    let fresh_suite = dir.path().join("fresh_suite.json");
    write_snapshot_suite(&suite, &fresh_suite).unwrap();
    let fresh_mode = std::fs::metadata(&fresh_suite)
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(fresh_mode, 0o600);
}
