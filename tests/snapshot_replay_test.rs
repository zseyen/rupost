use rupost::history::{RequestSnapshot, ResponseSnapshot, SnapshotEntry, SnapshotSuite};
use rupost::runner::ReplayExecutor;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_snapshot_recording_and_replaying() {
    // 1. Setup Mock Server
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/hello"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
             "message": "hello world"
        })))
        .mount(&mock_server)
        .await;

    // 2. 构造一个模拟快照 (假设在 example.com 录制)
    let temp_dir = TempDir::new().unwrap();
    let snapshot_file = temp_dir.path().join("snapshot.json");

    let entry = SnapshotEntry {
        id: "test-id-1".to_string(),
        request: RequestSnapshot {
            method: "GET".to_string(),
            url: "http://example.com/hello".to_string(), // 相对/别的域名
            headers: reqwest::header::HeaderMap::new(),
            body: None,
        },
        response: ResponseSnapshot {
            status: 200,
            headers: reqwest::header::HeaderMap::new(),
            body: r#"{"message": "hello world"}"#.to_string(),
        },
    };

    let suite = SnapshotSuite {
        timestamp: chrono::Utc::now(),
        source_file: Some("test_suite".to_string()),
        entries: vec![entry],
    };

    // 写入快照
    rupost::history::write_snapshot_suite(&suite, &snapshot_file).unwrap();
    assert!(snapshot_file.exists());

    // 3. 执行重放 (使用 --target 参数覆写 URL 指向本地 mock 端口)
    let replayer = ReplayExecutor::new(Some(mock_server.uri()), true);
    let report = replayer.replay_file(&snapshot_file).await.unwrap();

    assert_eq!(report.total, 1);
    assert_eq!(report.passed, 1);
    assert_eq!(report.failed, 0);

    // 4. 测试比对失败：使 mock 服务返回 500
    mock_server.reset().await;
    Mock::given(method("GET"))
        .and(path("/hello"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&mock_server)
        .await;

    let report_failed = replayer.replay_file(&snapshot_file).await.unwrap();
    assert_eq!(report_failed.total, 1);
    assert_eq!(report_failed.passed, 0);
    assert_eq!(report_failed.failed, 1);
}
