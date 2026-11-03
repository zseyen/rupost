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
    assert!(report_empty.entries.is_empty());
}

/// R6-a：混合成败（3 条 2 成 1 败）单次运行，entries 逐条分辨 + count 守恒 + error 条目二分归类
#[tokio::test]
async fn test_replay_mixed_results_entries_structured() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ok"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/bad"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&mock_server)
        .await;

    let temp_dir = TempDir::new().unwrap();
    let snapshot_file = temp_dir.path().join("mixed_snapshot.json");

    let make_entry = |id: &str, path_seg: &str, status: u16| SnapshotEntry {
        id: id.to_string(),
        request: RequestSnapshot {
            timeout: None,
            method: "GET".to_string(),
            url: format!("http://original.example.com/{}", path_seg),
            headers: reqwest::header::HeaderMap::new(),
            body: None,
        },
        response: ResponseSnapshot {
            status,
            headers: reqwest::header::HeaderMap::new(),
            body: String::new(),
        },
    };

    let suite = SnapshotSuite {
        timestamp: chrono::Utc::now(),
        source_file: Some("mixed_test".to_string()),
        entries: vec![
            make_entry("id-1", "ok", 200),
            make_entry("id-2", "bad", 200), // 期望 200 实得 503 → mismatch
            make_entry("id-3", "ok", 200),
        ],
    };
    write_snapshot_suite(&suite, &snapshot_file).unwrap();

    let replayer = ReplayExecutor::new(Some(mock_server.uri()), false);
    let report = replayer.replay_file(&snapshot_file).await.unwrap();

    // 聚合守恒：passed + failed = total = entries.len()
    assert_eq!(report.total, 3);
    assert_eq!(report.passed, 2);
    assert_eq!(report.failed, 1);
    assert_eq!(report.entries.len(), 3);
    assert_eq!(
        report.entries.iter().filter(|e| e.matched).count(),
        report.passed
    );

    // 失败条目精确定位（1-based request_number 与终端 #N 对齐）
    let failed_entry = &report.entries[1];
    assert_eq!(failed_entry.request_number, 2);
    assert_eq!(failed_entry.method, "GET");
    assert_eq!(failed_entry.expected_status, 200);
    assert_eq!(failed_entry.actual_status, Some(503));
    assert!(!failed_entry.matched);
    // mismatch 不置 error（传输层语义，expected/actual 由字段承载）
    assert!(failed_entry.error.is_none());
    // 失败条目 url 为 --target 覆写后实际请求 URL
    assert!(
        failed_entry.url.starts_with(mock_server.uri().as_str()),
        "entry url should be rewritten to target: {}",
        failed_entry.url
    );

    // 成功条目形态
    assert!(report.entries[0].matched);
    assert_eq!(report.entries[0].request_number, 1);
    assert_eq!(report.entries[0].actual_status, Some(200));
    assert!(report.entries[0].error.is_none());
}

/// R6-a：传输层失败（不可达端口）落条目——actual_status=None + error=Some，二分归 failed
#[tokio::test]
async fn test_replay_transport_error_entry_structured() {
    let temp_dir = TempDir::new().unwrap();
    let snapshot_file = temp_dir.path().join("dead_port_snapshot.json");

    let entry = SnapshotEntry {
        id: "id-dead".to_string(),
        request: RequestSnapshot {
            timeout: None,
            method: "GET".to_string(),
            url: "http://original.example.com/dead".to_string(),
            headers: reqwest::header::HeaderMap::new(),
            body: None,
        },
        response: ResponseSnapshot {
            status: 200,
            headers: reqwest::header::HeaderMap::new(),
            body: String::new(),
        },
    };
    let suite = SnapshotSuite {
        timestamp: chrono::Utc::now(),
        source_file: Some("dead_port_test".to_string()),
        entries: vec![entry],
    };
    write_snapshot_suite(&suite, &snapshot_file).unwrap();

    // 指向必然连接拒绝的端口（1 保留端口）
    let replayer = ReplayExecutor::new(Some("http://127.0.0.1:1".to_string()), false);
    let report = replayer.replay_file(&snapshot_file).await.unwrap();

    assert_eq!(report.total, 1);
    assert_eq!(report.passed, 0);
    assert_eq!(
        report.failed, 1,
        "error entry must be counted as failed (二分不三桶)"
    );

    let err_entry = &report.entries[0];
    assert_eq!(err_entry.request_number, 1);
    assert_eq!(
        err_entry.actual_status, None,
        "transport failure → actual_status None"
    );
    assert!(
        err_entry.error.is_some(),
        "transport failure carries error message"
    );
    assert!(!err_entry.matched);
}

fn rupost_bin() -> &'static str {
    env!("CARGO_BIN_EXE_rupost")
}

/// R6-b/c：CLI 子进程级验收——失败时 stdout 含 Failed entries 清单且 exit 1
#[tokio::test]
async fn test_replay_cli_exit_code_and_failed_entries_stdout() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ok"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/bad"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&mock_server)
        .await;

    let temp_dir = TempDir::new().unwrap();
    let snapshot_file = temp_dir.path().join("cli_mixed_snapshot.json");

    let make_entry = |id: &str, path_seg: &str| SnapshotEntry {
        id: id.to_string(),
        request: RequestSnapshot {
            timeout: None,
            method: "GET".to_string(),
            url: format!("http://original.example.com/{}", path_seg),
            headers: reqwest::header::HeaderMap::new(),
            body: None,
        },
        response: ResponseSnapshot {
            status: 200,
            headers: reqwest::header::HeaderMap::new(),
            body: String::new(),
        },
    };

    let suite = SnapshotSuite {
        timestamp: chrono::Utc::now(),
        source_file: Some("cli_mixed_test".to_string()),
        entries: vec![make_entry("id-1", "ok"), make_entry("id-2", "bad")],
    };
    write_snapshot_suite(&suite, &snapshot_file).unwrap();

    // 进程内 stdout 被 cargo test 捕获——CLI 可见面（Failed entries 块 + 退出码）必须走子进程断言
    let out = std::process::Command::new(rupost_bin())
        .args([
            "replay",
            snapshot_file.to_str().unwrap(),
            "--target",
            mock_server.uri().as_str(),
        ])
        .output()
        .expect("run rupost replay");

    assert_eq!(
        out.status.code(),
        Some(1),
        "failed replay must exit 1 for CI gating"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Failed entries:"), "got: {stdout}");
    assert!(
        stdout.contains("#2 GET"),
        "failure listed with 1-based #N: {stdout}"
    );
    assert!(
        stdout.contains("expected 200, got 503"),
        "mismatch detail visible: {stdout}"
    );
}

/// R6-b/c：全成功时 exit 0 且无 Failed entries 块（零噪音）
#[tokio::test]
async fn test_replay_cli_all_success_exit_zero_without_failed_block() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ok"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    let temp_dir = TempDir::new().unwrap();
    let snapshot_file = temp_dir.path().join("cli_ok_snapshot.json");

    let entry = SnapshotEntry {
        id: "id-ok".to_string(),
        request: RequestSnapshot {
            timeout: None,
            method: "GET".to_string(),
            url: "http://original.example.com/ok".to_string(),
            headers: reqwest::header::HeaderMap::new(),
            body: None,
        },
        response: ResponseSnapshot {
            status: 200,
            headers: reqwest::header::HeaderMap::new(),
            body: String::new(),
        },
    };
    let suite = SnapshotSuite {
        timestamp: chrono::Utc::now(),
        source_file: Some("cli_ok_test".to_string()),
        entries: vec![entry],
    };
    write_snapshot_suite(&suite, &snapshot_file).unwrap();

    let out = std::process::Command::new(rupost_bin())
        .args([
            "replay",
            snapshot_file.to_str().unwrap(),
            "--target",
            mock_server.uri().as_str(),
        ])
        .output()
        .expect("run rupost replay");

    assert_eq!(out.status.code(), Some(0), "all-success replay must exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("Failed entries:"),
        "zero failures must print no failed-entries block: {stdout}"
    );
}

/// T5 闭环：history export 产物可直接被 replay 加载并重放（--target 覆写，wiremock 回环）
#[tokio::test]
async fn test_history_export_then_replay_closed_loop() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/one"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/two"))
        .respond_with(ResponseTemplate::new(201))
        .mount(&mock_server)
        .await;

    // 手写 history.jsonl（HistoryEntry JSON 形态，快照期望状态 = 录制响应状态）
    let temp_dir = TempDir::new().unwrap();
    let hist_dir = temp_dir.path().join("hist");
    fs::create_dir_all(&hist_dir).unwrap();
    let history_file = hist_dir.join("history.jsonl");
    let line1 = serde_json::json!({
        "id": "h1", "timestamp": "2026-11-04T00:00:00Z", "duration_ms": 10,
        "request": {"method": "GET", "url": "http://origin.example.com/one", "headers": {}, "body": null},
        "source": "smoke",
        "response": {"status": 200, "headers": {}, "body": "resp one"}
    });
    let line2 = serde_json::json!({
        "id": "h2", "timestamp": "2026-11-04T00:00:01Z", "duration_ms": 10,
        "request": {"method": "POST", "url": "http://origin.example.com/two", "headers": {}, "body": "{\"k\":1}"},
        "source": "smoke",
        "response": {"status": 201, "headers": {}, "body": "resp two"}
    });
    fs::write(&history_file, format!("{line1}\n{line2}\n")).unwrap();

    let exported = temp_dir.path().join("exported.json");

    // 1. export（子进程 + RUPOST_HISTORY_DIR 隔离）
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        .env("RUPOST_HISTORY_DIR", hist_dir.to_str().unwrap())
        .args([
            "history",
            "export",
            "--last",
            "2",
            "-o",
            exported.to_str().unwrap(),
        ])
        .output()
        .expect("run history export");
    assert!(
        out.status.success(),
        "export failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // 2. replay 导出产物：两条期望状态均被 mock 满足 → exit 0
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        .args([
            "replay",
            exported.to_str().unwrap(),
            "--target",
            mock_server.uri().as_str(),
        ])
        .output()
        .expect("run replay of exported snapshot");
    let replay_stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(0),
        "exported snapshot must be replayable: {replay_stdout}"
    );
    // 条目数守恒可见：导出 2 条必须被真实消费（而非导出空套件也绿的盲区）
    assert!(
        replay_stdout.contains("Total: 2, Passed: 2"),
        "closed loop must consume both exported entries: {replay_stdout}"
    );
}

/// R6-a §4.2 补充定案守护：error 形态 duration_ms = 请求发起至失败的实际耗时（禁硬编码 0）——
/// 用 wiremock 2s 延迟 + 快照 100ms @timeout 造确定性超时形态，断言落在实际耗时量级区间
#[tokio::test]
async fn test_replay_error_entry_duration_reflects_actual_elapsed() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/slow"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("slow")
                .set_delay(Duration::from_secs(2)),
        )
        .mount(&mock_server)
        .await;

    let temp_dir = TempDir::new().unwrap();
    let snapshot_file = temp_dir.path().join("slow_timeout_snapshot.json");

    let entry = SnapshotEntry {
        id: "id-slow".to_string(),
        request: RequestSnapshot {
            timeout: Some(Duration::from_millis(100)),
            method: "GET".to_string(),
            url: "http://original.example.com/slow".to_string(),
            headers: reqwest::header::HeaderMap::new(),
            body: None,
        },
        response: ResponseSnapshot {
            status: 200,
            headers: reqwest::header::HeaderMap::new(),
            body: String::new(),
        },
    };
    let suite = SnapshotSuite {
        timestamp: chrono::Utc::now(),
        source_file: Some("slow_timeout_test".to_string()),
        entries: vec![entry],
    };
    write_snapshot_suite(&suite, &snapshot_file).unwrap();

    let replayer = ReplayExecutor::new(Some(mock_server.uri()), false);
    let report = replayer.replay_file(&snapshot_file).await.unwrap();

    assert_eq!(report.failed, 1);
    let err_entry = &report.entries[0];
    assert_eq!(err_entry.actual_status, None);
    // 100ms @timeout 触发：耗时必 ≥ 量级下限且远小于 2s mock 延迟
    // （上限 1500 对齐 test_replay_honors_recorded_timeout 的 3 倍余量哲学，防慢 CI 挂穿）
    assert!(
        err_entry.duration_ms >= 30 && err_entry.duration_ms <= 1500,
        "error entry duration must reflect actual elapsed (~100ms timeout), got {}",
        err_entry.duration_ms
    );
}
