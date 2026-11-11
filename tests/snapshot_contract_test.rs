//! 黑匣子快照契约与领域模型守护测试（拆分自超长存储文件，保持单文件轻量可维护）。

use rupost::history::model::{
    ExecutionSnapshotFile, MaskingMode, RequestSnapshot, RunnerFingerprint,
    SNAPSHOT_SCHEMA_VERSION, SocketDiagnostics, TimingWaterfallUs,
};
use rupost::history::storage::{
    read_execution_snapshot, save_batch_snapshot, write_execution_snapshot,
};
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
