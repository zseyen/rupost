use rupost::history::{
    RequestSnapshot, ResponseSnapshot, SnapshotEntry, SnapshotSuite, write_snapshot_suite,
};
use rupost::runner::ReplayExecutor;
use std::fs;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use wiremock::matchers::{body_string, method, path};
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
            timeout: None,
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
    write_snapshot_suite(&suite, &snapshot_file).unwrap();
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

#[tokio::test]
async fn test_replay_multi_entry_roundtrip_equivalence() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/a"))
        .respond_with(ResponseTemplate::new(200).set_body_string("resp_a"))
        .expect(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/b"))
        .and(body_string("{\"k\":\"v\"}"))
        .respond_with(ResponseTemplate::new(201).set_body_string("resp_b"))
        .expect(1)
        .mount(&mock_server)
        .await;

    let temp_dir = TempDir::new().unwrap();
    let snapshot_file = temp_dir.path().join("multi_snapshot.json");

    let entry_a = SnapshotEntry {
        id: "id-a".to_string(),
        request: RequestSnapshot {
            timeout: None,
            method: "GET".to_string(),
            url: "http://original.example.com/a".to_string(),
            headers: reqwest::header::HeaderMap::new(),
            body: None,
        },
        response: ResponseSnapshot {
            status: 200,
            headers: reqwest::header::HeaderMap::new(),
            body: "resp_a".to_string(),
        },
    };

    let entry_b = SnapshotEntry {
        id: "id-b".to_string(),
        request: RequestSnapshot {
            timeout: None,
            method: "POST".to_string(),
            url: "http://original.example.com/b".to_string(),
            headers: reqwest::header::HeaderMap::new(),
            body: Some("{\"k\":\"v\"}".to_string()),
        },
        response: ResponseSnapshot {
            status: 201,
            headers: reqwest::header::HeaderMap::new(),
            body: "resp_b".to_string(),
        },
    };

    let suite = SnapshotSuite {
        timestamp: chrono::Utc::now(),
        source_file: Some("multi_test".to_string()),
        entries: vec![entry_a, entry_b],
    };

    write_snapshot_suite(&suite, &snapshot_file).unwrap();

    let replayer = ReplayExecutor::new(Some(mock_server.uri()), false);
    let report = replayer.replay_file(&snapshot_file).await.unwrap();

    assert_eq!(report.total, 2);
    assert_eq!(report.passed, 2);
    assert_eq!(report.failed, 0);

    // 验证 mock_server 预期确实被逐条匹配
    mock_server.verify().await;
}

#[tokio::test]
async fn test_replay_honors_recorded_timeout() {
    let mock_server = MockServer::start().await;

    // mock 延迟 2 秒响应
    Mock::given(method("GET"))
        .and(path("/slow"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("slow response")
                .set_delay(Duration::from_secs(2)),
        )
        .mount(&mock_server)
        .await;

    let temp_dir = TempDir::new().unwrap();
    let snapshot_file = temp_dir.path().join("timeout_snapshot.json");

    // 快照记录了 500ms 超时
    let entry = SnapshotEntry {
        id: "id-timeout".to_string(),
        request: RequestSnapshot {
            timeout: Some(Duration::from_millis(500)),
            method: "GET".to_string(),
            url: "http://example.com/slow".to_string(),
            headers: reqwest::header::HeaderMap::new(),
            body: None,
        },
        response: ResponseSnapshot {
            status: 200,
            headers: reqwest::header::HeaderMap::new(),
            body: "slow response".to_string(),
        },
    };

    let suite = SnapshotSuite {
        timestamp: chrono::Utc::now(),
        source_file: Some("timeout_test".to_string()),
        entries: vec![entry],
    };

    write_snapshot_suite(&suite, &snapshot_file).unwrap();

    let replayer = ReplayExecutor::new(Some(mock_server.uri()), false);

    let start = Instant::now();
    let report = replayer.replay_file(&snapshot_file).await.unwrap();
    let duration = start.elapsed();

    // 必须被 500ms 超时切断并计为 failed
    assert_eq!(report.total, 1);
    assert_eq!(report.passed, 0);
    assert_eq!(report.failed, 1);

    // 计时行为学证明：若未透传 500ms 超时，则会等待至少 2s 或默认的 30s。此处必 < 1.5s
    assert!(
        duration < Duration::from_millis(1500),
        "Replay took {:?}, expected < 1500ms due to timeout enforcement",
        duration
    );
}

#[tokio::test]
async fn test_replay_corrupt_snapshot_readable_error() {
    let temp_dir = TempDir::new().unwrap();
    let replayer = ReplayExecutor::new(None, false);

    // 1. 坏输入一：纯文本非合法 JSON
    let path_plain = temp_dir.path().join("plain.txt");
    fs::write(&path_plain, "not a valid json snapshot").unwrap();
    let res_plain = replayer.replay_file(&path_plain).await;
    assert!(res_plain.is_err(), "Expected error for plain text file");

    // 2. 坏输入二：合法 JSON 但非 SnapshotSuite 结构
    let path_wrong_json = temp_dir.path().join("wrong.json");
    fs::write(&path_wrong_json, "{\"unexpected\": 123}").unwrap();
    let res_wrong_json = replayer.replay_file(&path_wrong_json).await;
    assert!(
        res_wrong_json.is_err(),
        "Expected error for invalid snapshot schema"
    );

    // 3. 坏输入三：结构合法但 entries 为空数组
    let path_empty = temp_dir.path().join("empty_suite.json");
    let suite = SnapshotSuite {
        timestamp: chrono::Utc::now(),
        source_file: Some("empty".to_string()),
        entries: vec![],
    };
    write_snapshot_suite(&suite, &path_empty).unwrap();
    let report_empty = replayer.replay_file(&path_empty).await.unwrap();
    assert_eq!(report_empty.total, 0);
    assert_eq!(report_empty.passed, 0);
    assert_eq!(report_empty.failed, 0);
}
