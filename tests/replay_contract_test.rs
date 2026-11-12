//! 批 3（replay-reuse B-2/B-5/B-6）：replay 消费契约收口守护。
//!
//! B-2：masked 快照警告"数据不完整"；truncated 条目跳过 body diff。
//! B-5：64MB 体积上限 + schema 主版本门（不可信输入防线，与 triage 对齐）。
//! B-6：黑匣子快照补 `@timeout` 往返（timeout_ms），重放复现原超时语义。

use rupost::history::{
    model::{ExecutionSnapshotFile, SnapshotRequest, SnapshotResponse},
    storage::write_execution_snapshot,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn base_file() -> ExecutionSnapshotFile {
    serde_json::from_str(
        r#"{
        "schema_version": "1.0",
        "generated_at": "2026-11-13T00:00:00Z",
        "environment": null,
        "runner_fingerprint": {"os":"os","arch":"arch","rupost_version":"0.2.0"},
        "masking": "none",
        "entries": []
    }"#,
    )
    .unwrap()
}

/// B-5：>64MB 快照必须拒绝（02 §3 不可信输入防线，与 triage 对齐）
#[tokio::test]
async fn test_replay_rejects_oversized_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let big = dir.path().join("big.json");
    std::fs::write(&big, " ".repeat(65 * 1024 * 1024)).unwrap();
    let out = tokio::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        .args([
            "replay",
            big.to_str().unwrap(),
            "--target",
            "http://127.0.0.1:1",
        ])
        .output()
        .await
        .unwrap();
    assert_ne!(out.status.code(), Some(0), "超限必须失败");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("64") || stderr.contains("过大") || stderr.contains("超过"),
        "错误必须指明体积上限: {stderr}"
    );
}

/// B-5：未来主版本快照必须显式拒绝（"1.x 均可消费"契约的另一半：2.0 静默错读 = 事故）
#[tokio::test]
async fn test_replay_rejects_future_schema_version() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("future.json");
    let mut file = base_file();
    file.schema_version = "2.0".to_string();
    rupost::history::storage::write_execution_snapshot(&file, &path).unwrap();
    let out = tokio::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        .args([
            "replay",
            path.to_str().unwrap(),
            "--target",
            "http://127.0.0.1:1",
        ])
        .output()
        .await
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("schema") || stderr.contains("版本"),
        "拒绝消息必须指明版本不兼容: {stderr}"
    );
}

/// B-2：masking=secrets 快照重放必须警告"数据不完整"（02 §4：验签/Diff 结论降级）
#[tokio::test]
async fn test_replay_masked_snapshot_warns_data_incomplete() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/x"))
        .respond_with(ResponseTemplate::new(200).set_body_string("whatever"))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("masked.json");
    let mut file = base_file();
    file.masking = rupost::history::model::MaskingMode::Secrets;
    file.entries
        .push(rupost::history::model::ExecutionSnapshotEntry {
            request_number: 1,
            name: Some("e1".to_string()),
            file_path: "t.http".to_string(),
            success: true,
            outcome: rupost::history::model::ExecutionOutcome::Pass,
            status: Some(200),
            duration_us: 100,
            plugin_duration_us: None,
            request: SnapshotRequest {
                method: "GET".to_string(),
                url: "http://original.example.com/x".to_string(),
                headers: vec![],
                body: None,
                timeout_ms: None,
            },
            response: Some(SnapshotResponse {
                status: 200,
                headers: vec![],
                body: "[REDACTED]".to_string(),
                body_truncated: false,
                body_sha256: None,
            }),
            error: None,
            timing_waterfall: None,
            timing_source: None,
            timing_attribution: None,
            socket_diagnostics: None,
            assertions: vec![],
            warnings: vec![],
        });
    write_execution_snapshot(&file, &path).unwrap();

    let out = tokio::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        .args([
            "replay",
            path.to_str().unwrap(),
            "--target",
            server.uri().as_str(),
            "--status-only",
        ])
        .output()
        .await
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("数据不完整"),
        "masked 快照重放必须警告验签/Diff 结论降级: {stdout}"
    );
}

/// B-2：body_truncated 条目必须跳过 body diff（截断 JSON 前缀必然解析失败 → 假失败）
#[tokio::test]
async fn test_replay_truncated_entry_skips_body_diff() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/big"))
        .respond_with(ResponseTemplate::new(200).set_body_string("A".repeat(64)))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("trunc.json");
    let mut file = base_file();
    file.entries
        .push(rupost::history::model::ExecutionSnapshotEntry {
            request_number: 1,
            name: Some("big".to_string()),
            file_path: "t.http".to_string(),
            success: true,
            outcome: rupost::history::model::ExecutionOutcome::Pass,
            status: Some(200),
            duration_us: 100,
            plugin_duration_us: None,
            request: SnapshotRequest {
                method: "GET".to_string(),
                url: "http://original.example.com/big".to_string(),
                headers: vec![],
                body: None,
                timeout_ms: None,
            },
            response: Some(SnapshotResponse {
                status: 200,
                headers: vec![],
                body: "AAAA".to_string(), // 截断形态：非完整 JSON 前缀
                body_truncated: true,
                body_sha256: None,
            }),
            error: None,
            timing_waterfall: None,
            timing_source: None,
            timing_attribution: None,
            socket_diagnostics: None,
            assertions: vec![],
            warnings: vec![],
        });
    write_execution_snapshot(&file, &path).unwrap();

    let out = tokio::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        .args([
            "replay",
            path.to_str().unwrap(),
            "--target",
            server.uri().as_str(),
        ])
        .output()
        .await
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "截断条目必须跳过 body diff（status 200 匹配即过）: stdout={stdout}"
    );
    assert!(stdout.contains("截断"), "跳过原因必须可见注明: {stdout}");
}
