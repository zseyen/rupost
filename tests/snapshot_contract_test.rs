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
        &[],
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
        &[],
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
        &[],
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
        &[],
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
            body_truncated: None,
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
        masking: None,
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

#[test]
fn test_redaction_contract_headers_query_body_and_ai_parity() {
    use rupost::ai::redactor::redact_and_truncate_body;
    use rupost::history::mask_execution_snapshot_entry;
    use rupost::history::model::{ExecutionOutcome, ExecutionSnapshotEntry, SnapshotResponse};
    use rupost::security::redact::mask_body_secrets;

    let req_body = r#"{
        "password": "pw1",
        "passwd": "pw2",
        "secret": "sec3",
        "token": "tok4",
        "access_token": "at5",
        "phone": 13812345678,
        "mobile": "13900001111",
        "id_card": "11010119900101123X",
        "nested": {
            "id_card": 110101199001011234,
            "mobile": 13700001111,
            "phone": "13600001111",
            "primary_key": 101,
            "design": "minimal",
            "assign": "bob",
            "signal": "ok",
            "keyboard": "qwerty"
        }
    }"#;

    let mut req_headers = reqwest::header::HeaderMap::new();
    req_headers.append("authorization", "Bearer auth_secret_1".parse().unwrap());
    req_headers.append(
        "authorization-ext",
        "Bearer auth_ext_secret_2".parse().unwrap(),
    );
    req_headers.append("cookie", "sid=cookie_secret_3".parse().unwrap());
    req_headers.append("x-sign", "sign_secret_4".parse().unwrap());
    req_headers.append("x-signature", "sig_secret_5".parse().unwrap());
    req_headers.append("content-type", "application/json".parse().unwrap());

    let mut resp_headers = reqwest::header::HeaderMap::new();
    resp_headers.append("set-cookie", "s1=resp_cookie_1; HttpOnly".parse().unwrap());
    resp_headers.append("set-cookie", "s2=resp_cookie_2; Path=/".parse().unwrap());
    resp_headers.append("content-type", "application/json".parse().unwrap());

    let mut legacy_entry = SnapshotEntry::new_success(
        "contract-redact".to_string(),
        RequestSnapshot {
            method: "POST".to_string(),
            url: "https://api.example.com/submit?token=qt1&access_token=qat2&sign=qs3&sig=qsig4&key=qk5&secret=qsec6&page=3&next_token=cursor9&design=v1&signal=true".to_string(),
            headers: req_headers,
            body: Some(req_body.to_string()),
            timeout: None,
        },
        ResponseSnapshot {
            status: 200,
            headers: resp_headers,
            body: req_body.to_string(),
        body_truncated: None,
        },
    );
    mask_snapshot_entry(&mut legacy_entry);

    // 1. Request headers
    assert_eq!(
        legacy_entry
            .request
            .headers
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap(),
        "Bearer [REDACTED]"
    );
    assert_eq!(
        legacy_entry
            .request
            .headers
            .get("authorization-ext")
            .unwrap()
            .to_str()
            .unwrap(),
        "Bearer [REDACTED]"
    );
    assert_eq!(
        legacy_entry
            .request
            .headers
            .get("cookie")
            .unwrap()
            .to_str()
            .unwrap(),
        "[REDACTED]"
    );
    assert_eq!(
        legacy_entry
            .request
            .headers
            .get("x-sign")
            .unwrap()
            .to_str()
            .unwrap(),
        "[REDACTED]"
    );
    assert_eq!(
        legacy_entry
            .request
            .headers
            .get("x-signature")
            .unwrap()
            .to_str()
            .unwrap(),
        "[REDACTED]"
    );
    assert_eq!(
        legacy_entry
            .request
            .headers
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "application/json"
    );

    // 2. Response headers (multi-value Set-Cookie)
    let set_cookies: Vec<&str> = legacy_entry
        .response
        .as_ref()
        .unwrap()
        .headers
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap())
        .collect();
    assert_eq!(set_cookies, vec!["[REDACTED]", "[REDACTED]"]);

    // 3. URL query (sensitive masked + non-sensitive preserved)
    for leaked in ["qt1", "qat2", "qs3", "qsig4", "qk5", "qsec6"] {
        assert!(
            !legacy_entry.request.url.contains(leaked),
            "query secret {leaked} leaked: {}",
            legacy_entry.request.url
        );
    }
    for kept in ["page=3", "next_token=cursor9", "design=v1", "signal=true"] {
        assert!(
            legacy_entry.request.url.contains(kept),
            "non-sensitive query {kept} missing: {}",
            legacy_entry.request.url
        );
    }

    // 4. Body JSON (String + Number + nested) + AI redactor parity
    let masked_body = legacy_entry.request.body.as_deref().unwrap();
    let ai_body = redact_and_truncate_body(req_body, 4096);
    let exp_body = mask_body_secrets(req_body);
    let masked_json: serde_json::Value = serde_json::from_str(masked_body).unwrap();
    let ai_json: serde_json::Value = serde_json::from_str(&ai_body).unwrap();
    let exp_json: serde_json::Value = serde_json::from_str(&exp_body).unwrap();
    assert_eq!(masked_json, exp_json);
    assert_eq!(ai_json, exp_json);

    for secret_key in ["password", "passwd", "secret", "token", "access_token"] {
        assert_eq!(masked_json[secret_key], "[REDACTED]");
    }
    assert_eq!(masked_json["phone"], "***");
    assert_eq!(masked_json["mobile"], "***");
    assert_eq!(masked_json["id_card"], "***");
    assert_eq!(masked_json["nested"]["id_card"], "***");
    assert_eq!(masked_json["nested"]["mobile"], "***");
    assert_eq!(masked_json["nested"]["phone"], "***");
    assert_eq!(masked_json["nested"]["primary_key"], 101);
    assert_eq!(masked_json["nested"]["design"], "minimal");
    assert_eq!(masked_json["nested"]["assign"], "bob");
    assert_eq!(masked_json["nested"]["signal"], "ok");
    assert_eq!(masked_json["nested"]["keyboard"], "qwerty");

    // 5. ExecutionSnapshotEntry surface check (single Set-Cookie + headers + body)
    let mut exec_entry = ExecutionSnapshotEntry {
        request_number: 1,
        name: None,
        file_path: "t.http".to_string(),
        success: true,
        outcome: ExecutionOutcome::Pass,
        status: Some(200),
        duration_us: 100,
        plugin_duration_us: None,
        request: rupost::history::model::SnapshotRequest {
            method: "POST".to_string(),
            url: "https://api.example.com/submit?sign=qs3&page_token=pt1".to_string(),
            headers: vec![
                ("Authorization-Ext".to_string(), "Bearer ext9".to_string()),
                ("X-Sign".to_string(), "sig9".to_string()),
                ("Content-Type".to_string(), "application/json".to_string()),
            ],
            body: Some(req_body.to_string()),
            timeout_ms: None,
        },
        response: Some(SnapshotResponse {
            status: 200,
            headers: vec![("Set-Cookie".to_string(), "sid=single_cookie".to_string())],
            body: req_body.to_string(),
            body_truncated: false,
            body_sha256: None,
        }),
        error: None,
        timing_waterfall: None,
        timing_source: None,
        timing_attribution: None,
        socket_diagnostics: None,
        assertions: vec![],
        warnings: vec!["X-Sign: warn_sig_1234".to_string()],
    };
    mask_execution_snapshot_entry(&mut exec_entry, &[]);
    assert_eq!(exec_entry.request.headers[0].1, "Bearer [REDACTED]");
    assert_eq!(exec_entry.request.headers[1].1, "[REDACTED]");
    assert_eq!(exec_entry.request.headers[2].1, "application/json");
    assert_eq!(
        exec_entry.response.as_ref().unwrap().headers[0].1,
        "[REDACTED]"
    );
    assert!(exec_entry.request.url.contains("page_token=pt1"));
    assert!(!exec_entry.request.url.contains("qs3"));
    assert!(!exec_entry.warnings[0].contains("warn_sig_1234"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_default_save_snapshot_without_debug_records_connector_timing_and_keepalive_nulls() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                while let Ok(n) = socket.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                    let resp =
                        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: keep-alive\r\n\r\nok";
                    if socket.write_all(resp.as_bytes()).await.is_err() {
                        break;
                    }
                }
            });
        }
    });

    let dir = tempfile::tempdir().unwrap();
    let http_file = dir.path().join("m7_timing.http");
    let snap_file = dir.path().join("snap.json");
    let content = format!(
        "### Cold Request\nGET http://localhost:{}/first\n\n### Pooled Request\nGET http://localhost:{}/second\n",
        addr.port(),
        addr.port()
    );
    std::fs::write(&http_file, content).unwrap();

    let mut cmd = tokio::process::Command::new(env!("CARGO_BIN_EXE_rupost"));
    cmd.arg("test")
        .arg(&http_file)
        .arg("--save-snapshot")
        .arg(&snap_file);
    for proxy_var in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        cmd.env_remove(proxy_var);
    }
    let output = cmd.output().await.unwrap();
    assert!(
        output.status.success(),
        "rupost test --save-snapshot failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let raw_json = std::fs::read_to_string(&snap_file).unwrap();
    let raw_val: serde_json::Value = serde_json::from_str(&raw_json).unwrap();
    let entries = raw_val["entries"].as_array().expect("entries array");
    assert_eq!(entries.len(), 2, "must have 2 entries");

    // 1. Cold request (without --debug): non-null timing_waterfall, timing_source="connector", timing_attribution="exact"
    let e0 = &entries[0];
    assert!(
        e0["timing_waterfall"].is_object(),
        "default --save-snapshot without --debug must produce non-null timing_waterfall: {e0}"
    );
    assert!(
        e0["timing_waterfall"]["dns_lookup_us"].is_u64(),
        "cold hostname request must have dns_lookup_us: {e0}"
    );
    assert!(
        e0["timing_waterfall"]["tcp_connect_us"].is_u64(),
        "cold request must have tcp_connect_us: {e0}"
    );
    assert_eq!(e0["timing_source"], "connector");
    assert_eq!(e0["timing_attribution"], "exact");

    // 2. Pooled keep-alive request: dns_lookup_us=null, tcp_connect_us=null, ttfb_us present
    let e1 = &entries[1];
    assert!(
        e1["timing_waterfall"].is_object(),
        "pooled request must still record timing_waterfall: {e1}"
    );
    assert!(
        e1["timing_waterfall"]["dns_lookup_us"].is_null(),
        "pooled request must have dns_lookup_us: null: {e1}"
    );
    assert!(
        e1["timing_waterfall"]["tcp_connect_us"].is_null(),
        "pooled request must have tcp_connect_us: null: {e1}"
    );
    assert!(
        e1["timing_waterfall"]["ttfb_us"].is_u64(),
        "pooled request must have ttfb_us: {e1}"
    );
    assert_eq!(e1["timing_source"], "connector");
    assert_eq!(e1["timing_attribution"], "exact");
}

/// P1-1（M7-A 语义边界裁决）：并行模式下 connector 观测的 dns/tcp 可能因连接池
/// 竞争跨请求错位 → 快照 timing_attribution 必须标 approximate（诚实契约），
/// timing_source 保持 connector；串行路径保持 exact（R1 既有守护不回退）。
#[tokio::test]
async fn test_parallel_mode_marks_connector_timing_approximate() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                while let Ok(n) = socket.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                    let resp =
                        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: keep-alive\r\n\r\nok";
                    if socket.write_all(resp.as_bytes()).await.is_err() {
                        break;
                    }
                }
            });
        }
    });

    let dir = tempfile::tempdir().unwrap();
    let http_file = dir.path().join("par_timing.http");
    let snap_file = dir.path().join("par_snap.json");
    let content = format!(
        "### Parallel One\nGET http://localhost:{}/p1\n\n### Parallel Two\nGET http://localhost:{}/p2\n",
        addr.port(),
        addr.port()
    );
    std::fs::write(&http_file, content).unwrap();

    let mut cmd = tokio::process::Command::new(env!("CARGO_BIN_EXE_rupost"));
    cmd.arg("test")
        .arg(&http_file)
        .arg("--mode")
        .arg("parallel")
        .arg("--save-snapshot")
        .arg(&snap_file);
    for proxy_var in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        cmd.env_remove(proxy_var);
    }
    let output = cmd.output().await.unwrap();
    assert!(
        output.status.success(),
        "parallel snapshot run failed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let raw_val: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&snap_file).unwrap()).unwrap();
    let entries = raw_val["entries"].as_array().expect("entries array");
    assert_eq!(entries.len(), 2);
    for entry in entries {
        assert_eq!(
            entry["timing_source"], "connector",
            "并行请求仍为 connector 真实测量"
        );
        assert_eq!(
            entry["timing_attribution"], "approximate",
            "并行模式 connector dns/tcp 观测必须诚实标注 approximate: {}",
            entry["name"]
        );
    }
}

/// P1-2（02 §2:135 契约兑现）：`--timing off` → 快照 timing_waterfall=null、
/// timing_source=none（显式记录"未采集"，与字段缺省 null 区分）。
#[tokio::test]
async fn test_timing_off_writes_null_waterfall_and_none_source() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                while let Ok(n) = socket.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                    let resp =
                        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: keep-alive\r\n\r\nok";
                    if socket.write_all(resp.as_bytes()).await.is_err() {
                        break;
                    }
                }
            });
        }
    });

    let dir = tempfile::tempdir().unwrap();
    let http_file = dir.path().join("timing_off.http");
    let snap_file = dir.path().join("off_snap.json");
    let content = format!("### Off Case\nGET http://localhost:{}/off\n", addr.port());
    std::fs::write(&http_file, content).unwrap();

    let mut cmd = tokio::process::Command::new(env!("CARGO_BIN_EXE_rupost"));
    cmd.arg("test")
        .arg(&http_file)
        .arg("--timing")
        .arg("off")
        .arg("--save-snapshot")
        .arg(&snap_file);
    let output = cmd.output().await.unwrap();
    assert!(
        output.status.success(),
        "timing off run failed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let raw_val: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&snap_file).unwrap()).unwrap();
    let entry = &raw_val["entries"][0];
    assert!(
        entry["timing_waterfall"].is_null(),
        "--timing off 必须产出 timing_waterfall=null: {}",
        entry
    );
    assert_eq!(
        entry["timing_source"], "none",
        "--timing off 必须显式记录 timing_source=none"
    );
    assert!(
        entry["timing_attribution"].is_null(),
        "--timing off 无归属标记"
    );
}

/// P1-2 防误用对照：`--timing <未知模式>` 属语法错误，必须 exit 2。
#[tokio::test]
async fn test_timing_unknown_mode_is_syntax_error_exit_2() {
    let dir = tempfile::tempdir().unwrap();
    let http_file = dir.path().join("x.http");
    std::fs::write(&http_file, "### x\nGET http://127.0.0.1:1/x\n").unwrap();

    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        .arg("test")
        .arg(&http_file)
        .arg("--timing")
        .arg("turbo")
        .output()
        .await
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(2),
        "未知 --timing 模式必须 exit 2: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--timing") && stderr.contains("off"),
        "错误消息必须指明合法值: {stderr}"
    );
}

/// S-3（免重填审查）：`--mask-secret-value` 按值掩码独立于 `--mask-secrets` 生效——
/// 词表外的自定义 header（x-vault-ns 类）携带的密钥值必须被替换为 [REDACTED]。
#[tokio::test]
async fn test_mask_secret_value_hits_custom_headers() {
    let dir = tempfile::tempdir().unwrap();
    let snap = dir.path().join("snap_sv.json");
    let resp = Response::new(
        200,
        reqwest::header::HeaderMap::new(),
        "{}".to_string(),
        Duration::from_millis(5),
        Duration::from_millis(5),
        Duration::ZERO,
    )
    .unwrap();
    let mut result = TestResult::success(
        1,
        Some("vault".to_string()),
        "GET".to_string(),
        "https://vault.example.com/x?ns=ns-live-777".to_string(),
        resp,
    );
    result.request = Some(RequestSnapshot {
        method: "GET".to_string(),
        url: "https://vault.example.com/x?ns=ns-live-777".to_string(),
        headers: {
            let mut m = reqwest::header::HeaderMap::new();
            m.insert(
                "x-vault-ns",
                reqwest::header::HeaderValue::from_static("ns-live-777"),
            );
            m
        },
        body: None,
        timeout: None,
    });
    let batch = vec![(PathBuf::from("t.http"), vec![result])];
    // 只传按值掩码（mask_secrets=false）→ 独立生效
    save_batch_snapshot(
        &batch,
        &["t.http".to_string()],
        &snap,
        false,
        &["ns-live-777".to_string()],
        None,
        rupost::history::model::DEFAULT_MAX_SNAPSHOT_BODY_BYTES,
    )
    .unwrap();
    let content = std::fs::read_to_string(&snap).unwrap();
    assert!(
        !content.contains("ns-live-777"),
        "词表外自定义 header 的密钥值必须被按值掩码: {content}"
    );
    assert!(content.contains("[REDACTED]"));
    let snap_val: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(
        snap_val["masking"], "secrets",
        "值掩码独立生效时 masking 标记必须联动 secrets"
    );
}
