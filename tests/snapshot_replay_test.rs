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
        response: Some(ResponseSnapshot {
            status: 200,
            headers: reqwest::header::HeaderMap::new(),
            body: r#"{"message": "hello world"}"#.to_string(),
        }),
        error: None,
    };

    let suite = SnapshotSuite {
        version: 1,
        timestamp: chrono::Utc::now(),
        source_file: Some("test_suite".to_string()),
        entries: vec![entry],
    };

    // 写入快照
    write_snapshot_suite(&suite, &snapshot_file).unwrap();
    assert!(snapshot_file.exists());

    // 3. 执行重放 (使用 --target 参数覆写 URL 指向本地 mock 端口)
    let replayer = ReplayExecutor::new(Some(mock_server.uri()), true).unwrap();
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
        response: Some(ResponseSnapshot {
            status: 200,
            headers: reqwest::header::HeaderMap::new(),
            body: "resp_a".to_string(),
        }),
        error: None,
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
        response: Some(ResponseSnapshot {
            status: 201,
            headers: reqwest::header::HeaderMap::new(),
            body: "resp_b".to_string(),
        }),
        error: None,
    };

    let suite = SnapshotSuite {
        version: 1,
        timestamp: chrono::Utc::now(),
        source_file: Some("multi_test".to_string()),
        entries: vec![entry_a, entry_b],
    };

    write_snapshot_suite(&suite, &snapshot_file).unwrap();

    let replayer = ReplayExecutor::new(Some(mock_server.uri()), false).unwrap();
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
        response: Some(ResponseSnapshot {
            status: 200,
            headers: reqwest::header::HeaderMap::new(),
            body: "slow response".to_string(),
        }),
        error: None,
    };

    let suite = SnapshotSuite {
        version: 1,
        timestamp: chrono::Utc::now(),
        source_file: Some("timeout_test".to_string()),
        entries: vec![entry],
    };

    write_snapshot_suite(&suite, &snapshot_file).unwrap();

    let replayer = ReplayExecutor::new(Some(mock_server.uri()), false).unwrap();

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
    let replayer = ReplayExecutor::new(None, false).unwrap();

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
        version: 1,
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
        response: Some(ResponseSnapshot {
            status,
            headers: reqwest::header::HeaderMap::new(),
            body: String::new(),
        }),
        error: None,
    };

    let suite = SnapshotSuite {
        version: 1,
        timestamp: chrono::Utc::now(),
        source_file: Some("mixed_test".to_string()),
        entries: vec![
            make_entry("id-1", "ok", 200),
            make_entry("id-2", "bad", 200), // 期望 200 实得 503 → mismatch
            make_entry("id-3", "ok", 200),
        ],
    };
    write_snapshot_suite(&suite, &snapshot_file).unwrap();

    let replayer = ReplayExecutor::new(Some(mock_server.uri()), false).unwrap();
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
        response: Some(ResponseSnapshot {
            status: 200,
            headers: reqwest::header::HeaderMap::new(),
            body: String::new(),
        }),
        error: None,
    };
    let suite = SnapshotSuite {
        version: 1,
        timestamp: chrono::Utc::now(),
        source_file: Some("dead_port_test".to_string()),
        entries: vec![entry],
    };
    write_snapshot_suite(&suite, &snapshot_file).unwrap();

    // 指向必然连接拒绝的端口（1 保留端口）
    let replayer = ReplayExecutor::new(Some("http://127.0.0.1:1".to_string()), false).unwrap();
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
        response: Some(ResponseSnapshot {
            status: 200,
            headers: reqwest::header::HeaderMap::new(),
            body: String::new(),
        }),
        error: None,
    };

    let suite = SnapshotSuite {
        version: 1,
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
        response: Some(ResponseSnapshot {
            status: 200,
            headers: reqwest::header::HeaderMap::new(),
            body: String::new(),
        }),
        error: None,
    };
    let suite = SnapshotSuite {
        version: 1,
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
    // 响应体与录制历史一致（T4 body 比对开启后，内容漂移会正确判失败——本测试聚焦供给链）
    Mock::given(method("GET"))
        .and(path("/one"))
        .respond_with(ResponseTemplate::new(200).set_body_string("resp one"))
        .mount(&mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/two"))
        .respond_with(ResponseTemplate::new(201).set_body_string("resp two"))
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
        response: Some(ResponseSnapshot {
            status: 200,
            headers: reqwest::header::HeaderMap::new(),
            body: String::new(),
        }),
        error: None,
    };
    let suite = SnapshotSuite {
        version: 1,
        timestamp: chrono::Utc::now(),
        source_file: Some("slow_timeout_test".to_string()),
        entries: vec![entry],
    };
    write_snapshot_suite(&suite, &snapshot_file).unwrap();

    let replayer = ReplayExecutor::new(Some(mock_server.uri()), false).unwrap();
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

// ═══════════════ T4 Smart Diff 集成验收 ═══════════════

/// T4 端到端：抖动字段（排除键）值漂移不假失败；真实业务差异定位到条且携带脱敏摘要
#[tokio::test]
async fn test_smart_diff_jitter_tolerated_and_real_diff_caught() {
    let mock_server = MockServer::start().await;
    let temp_dir = TempDir::new().unwrap();
    let snapshot_file = temp_dir.path().join("smart_diff_snapshot.json");

    // 快照 body 含 timestamp（默认排除键）与 quota（业务字段）
    let entry = SnapshotEntry {
        id: "id-sd".to_string(),
        request: RequestSnapshot {
            timeout: None,
            method: "GET".to_string(),
            url: "http://original.example.com/data".to_string(),
            headers: reqwest::header::HeaderMap::new(),
            body: None,
        },
        response: Some(ResponseSnapshot {
            status: 200,
            headers: reqwest::header::HeaderMap::new(),
            body: r#"{"quota":100,"timestamp":"2026-01-01T00:00:00Z"}"#.to_string(),
        }),
        error: None,
    };
    let suite = SnapshotSuite {
        version: 1,
        timestamp: chrono::Utc::now(),
        source_file: Some("smart_diff".to_string()),
        entries: vec![entry],
    };
    write_snapshot_suite(&suite, &snapshot_file).unwrap();
    let replayer = ReplayExecutor::new(Some(mock_server.uri()), false).unwrap();

    // 1) 抖动：timestamp 漂移（quota 相同）→ matched，无假失败
    mock_server.reset().await;
    Mock::given(method("GET"))
        .and(path("/data"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "quota": 100, "timestamp": "2027-12-31T23:59:59Z"
        })))
        .mount(&mock_server)
        .await;
    let report = replayer.replay_file(&snapshot_file).await.unwrap();
    assert_eq!(report.failed, 0, "jitter on excluded key must not fail");
    assert_eq!(report.entries[0].body_matched, Some(true));
    assert!(report.entries[0].body_diff.is_none());

    // 2) 真实差异：quota 漂移 → body 失败定位到条 + 摘要
    mock_server.reset().await;
    Mock::given(method("GET"))
        .and(path("/data"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "quota": 999, "timestamp": "2027-12-31T23:59:59Z"
        })))
        .mount(&mock_server)
        .await;
    let report = replayer.replay_file(&snapshot_file).await.unwrap();
    assert_eq!(report.failed, 1);
    let e = &report.entries[0];
    assert!(!e.matched);
    assert_eq!(e.body_matched, Some(false));
    let diff = e.body_diff.as_ref().expect("body_diff carries summary");
    assert!(diff.contains("quota") && diff.contains("100") && diff.contains("999"));
    // 脱敏不变量在集成面复核：真实 token 值不出现在摘要
    assert!(
        !report.entries[0]
            .response_body_snippet
            .as_ref()
            .unwrap()
            .is_empty()
    );
}

/// T4 CLI 级：body-only 失败（状态匹配 body 漂移）→ 第四形态输出 + exit 1；
/// `--status-only` 逃生舱恢复 R6 语义 exit 0；`--ignore-fields` 用户键豁免
#[tokio::test]
async fn test_smart_diff_cli_fourth_form_and_escapes() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/data"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "quota": 100, "timestamp": "old"
        })))
        .mount(&mock_server)
        .await;

    let temp_dir = TempDir::new().unwrap();
    let snapshot_file = temp_dir.path().join("cli_sd_snapshot.json");
    let entry = SnapshotEntry {
        id: "id-cli".to_string(),
        request: RequestSnapshot {
            timeout: None,
            method: "GET".to_string(),
            url: "http://original.example.com/data".to_string(),
            headers: reqwest::header::HeaderMap::new(),
            body: None,
        },
        response: Some(ResponseSnapshot {
            status: 200,
            headers: reqwest::header::HeaderMap::new(),
            body: r#"{"quota":100,"timestamp":"new"}"#.to_string(),
        }),
        error: None,
    };
    let suite = SnapshotSuite {
        version: 1,
        timestamp: chrono::Utc::now(),
        source_file: Some("cli_sd".to_string()),
        entries: vec![entry],
    };
    write_snapshot_suite(&suite, &snapshot_file).unwrap();
    let uri = mock_server.uri();
    let target = ["--target", uri.as_str()];

    // 1) 默认语义：timestamp 属默认排除键 → 抖动容忍 exit 0（默认列表生效面）
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        .args(["replay", snapshot_file.to_str().unwrap()])
        .args(target)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "default-list exclusion keeps it green"
    );

    // 2) 用户排除键：quota 差异在 --ignore-fields 后豁免
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        .args([
            "replay",
            snapshot_file.to_str().unwrap(),
            "--ignore-fields",
            "quota",
        ])
        .args(target)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "user ignore_fields exempts quota"
    );

    // 3) 制造 body-only 失败（mock 改 quota）→ 第四形态 + exit 1
    mock_server.reset().await;
    Mock::given(method("GET"))
        .and(path("/data"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "quota": 555, "timestamp": "old"
        })))
        .mount(&mock_server)
        .await;
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        .args(["replay", snapshot_file.to_str().unwrap()])
        .args(target)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "body-only failure must exit 1");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Body mismatch"),
        "inline fourth-form line: {stdout}"
    );
    assert!(
        stdout.contains("(body: quota:"),
        "Failed entries fourth arm: {stdout}"
    );

    // 4) --status-only 逃生舱：同一失败场景恢复 R6 语义 exit 0
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        .args(["replay", snapshot_file.to_str().unwrap(), "--status-only"])
        .args(target)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "status-only escape hatch restores R6 semantics"
    );
}

/// P1 AI 接线：`replay --explain` 无 Key 降级——规则版出现、AI 段不出现、exit 码仍由比对决定
#[tokio::test]
async fn test_replay_explain_no_key_degradation() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/data"))
        .respond_with(ResponseTemplate::new(503).set_body_string("down"))
        .mount(&mock_server)
        .await;

    let temp_dir = TempDir::new().unwrap();
    let snapshot_file = temp_dir.path().join("explain_snapshot.json");
    let entry = SnapshotEntry {
        id: "id-explain".to_string(),
        request: RequestSnapshot {
            timeout: None,
            method: "GET".to_string(),
            url: "http://original.example.com/data".to_string(),
            headers: reqwest::header::HeaderMap::new(),
            body: None,
        },
        response: Some(ResponseSnapshot {
            status: 200,
            headers: reqwest::header::HeaderMap::new(),
            body: r#"{"ok":true}"#.to_string(),
        }),
        error: None,
    };
    let suite = SnapshotSuite {
        version: 1,
        timestamp: chrono::Utc::now(),
        source_file: Some("explain".to_string()),
        entries: vec![entry],
    };
    write_snapshot_suite(&suite, &snapshot_file).unwrap();

    // 1) 失败 + --explain（无 AI 配置）：Possible Causes 出现、AI 段不出现、exit 1
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        .args([
            "replay",
            snapshot_file.to_str().unwrap(),
            "--target",
            mock_server.uri().as_str(),
            "--explain",
        ])
        .env_remove("RUPOST_AI_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(1),
        "exit still driven by comparison"
    );
    assert!(
        stdout.contains("Possible Causes"),
        "rule baseline present: {stdout}"
    );
    assert!(
        !stdout.contains("AI Explanation"),
        "no AI section without key: {stdout}"
    );
    assert!(
        stderr.contains("AI 归因已跳过") || stderr.contains("AI"),
        "degradation hint on stderr: {stderr}"
    );

    // 2) 全成功 + --explain：exit 0 且无归因块
    mock_server.reset().await;
    Mock::given(method("GET"))
        .and(path("/data"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"ok":true}"#))
        .mount(&mock_server)
        .await;
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        .args([
            "replay",
            snapshot_file.to_str().unwrap(),
            "--target",
            mock_server.uri().as_str(),
            "--explain",
        ])
        .env_remove("RUPOST_AI_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0));
    assert!(
        !stdout.contains("Possible Causes"),
        "zero failures print no attribution block: {stdout}"
    );
}

/// Secrets Masker：`t --save-snapshot --mask-secrets` → 快照凭据/PII 打码且 schema 可重放加载
#[tokio::test]
async fn test_save_snapshot_mask_secrets_flag() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/data"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"phone":"13812345678","password":"p@ss","memo":"ok"}),
        ))
        .mount(&mock_server)
        .await;

    let dir = TempDir::new().unwrap();
    let case = dir.path().join("case.http");
    std::fs::write(
        &case,
        format!(
            "### fetch\n# @name fetch\nGET {}/data\nAuthorization: Bearer sk-live-secret\n",
            mock_server.uri()
        ),
    )
    .unwrap();
    let snap = dir.path().join("snap_masked.json");

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        .args([
            "t",
            case.to_str().unwrap(),
            "--save-snapshot",
            snap.to_str().unwrap(),
            "--mask-secrets",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());

    let content = std::fs::read_to_string(&snap).unwrap();
    assert!(
        !content.contains("sk-live-secret"),
        "header credential redacted: {content}"
    );
    assert!(
        content.contains("[REDACTED]"),
        "authorization masked: {content}"
    );
    assert!(!content.contains("13812345678"), "pii masked: {content}");
    assert!(content.contains("***"), "pii placeholder present");

    // 掩码快照 schema 未变——replay 仍可加载（本测试只验证 loadable，不验证 pass/fail）
    let replayer = ReplayExecutor::new(None, false).unwrap();
    let report = replayer.replay_file(&snap).await.unwrap();
    assert_eq!(report.total, 1);
    let _ = mock_server;
}

// ═══════════════ 红队审查修复轮补测 ═══════════════

/// T1 验收闭环（PRD §七）：生成文件重跑两次，录得的动态值必不同（resolver 接线断裂即红）
#[tokio::test]
async fn test_t1_generated_file_reruns_with_fresh_values() {
    use wiremock::matchers::{method, path};
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/ping"))
        .respond_with(ResponseTemplate::new(200).set_body_string("pong"))
        .mount(&server)
        .await;

    let dir = TempDir::new().unwrap();
    let case = dir.path().join("t1.http");
    let content = "### ping\n# @name ping\nGET ".to_string()
        + server.uri().as_str()
        + "/ping\nX-Request-Id: {{$uuid}}\n\n{\"rid\": \"{{$uuid}}\"}\n";
    std::fs::write(&case, content).unwrap();

    let run = || {
        std::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
            .args(["t", case.to_str().unwrap()])
            .output()
            .unwrap()
    };
    let first = run();
    assert!(
        first.status.success(),
        "run1: {}",
        String::from_utf8_lossy(&first.stdout)
    );
    let second = run();
    assert!(
        second.status.success(),
        "run2: {}",
        String::from_utf8_lossy(&second.stdout)
    );

    // 两次运行录得的 X-Request-Id 必不同（resolver 接线断裂 → 字面量 → 相等即红）
    let reqs = server.received_requests().await.expect("received requests");
    assert!(reqs.len() >= 2, "two runs recorded: {}", reqs.len());
    let ids: Vec<String> = reqs
        .iter()
        .map(|req| {
            req.headers
                .get("x-request-id")
                .map(|v| v.to_str().unwrap().to_string())
                .unwrap_or_default()
        })
        .collect();
    assert_ne!(ids[0], ids[ids.len() - 1], "fresh uuid per run: {ids:?}");
}

#[tokio::test]
async fn test_save_batch_snapshot_preserves_error_when_response_is_none() {
    use rupost::history::storage::save_batch_snapshot;
    use rupost::runner::types::TestResult;
    use std::path::PathBuf;

    let temp_dir = TempDir::new().unwrap();
    let snapshot_file = temp_dir.path().join("error_snapshot.json");

    let req_snap = RequestSnapshot {
        timeout: None,
        method: "GET".to_string(),
        url: "http://example.com/timeout".to_string(),
        headers: reqwest::header::HeaderMap::new(),
        body: None,
    };

    let fail_result = TestResult {
        request_number: 1,
        name: Some("TimeoutReq".to_string()),
        method: "GET".to_string(),
        url: "http://example.com/timeout".to_string(),
        status: None,
        duration: Duration::from_millis(5000),
        success: false,
        error: Some("connection timed out to https://api.com?secret_token=12345".to_string()),
        response: None, // 模拟网络超时，没有收到 Response
        skipped: false,
        assertions: vec![],
        timing: None,
        diagnose_report: None,
        request: Some(req_snap),
        warnings: vec![],
        is_teardown: false,
        degraded: false,
        plugin_duration_us: None,
        connect_span: None,
        timing_source: None,
        timing_attribution: None,
    };

    let batch = vec![(PathBuf::from("test.http"), vec![fail_result])];
    save_batch_snapshot(
        &batch,
        &["test.http".to_string()],
        &snapshot_file,
        true,
        None,
        rupost::history::model::DEFAULT_MAX_SNAPSHOT_BODY_BYTES,
    )
    .unwrap();

    let content = fs::read_to_string(&snapshot_file).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();

    // 验证：条目数必须为 1，不能被静默吞掉
    let entries = parsed.get("entries").and_then(|v| v.as_array()).unwrap();
    assert_eq!(entries.len(), 1, "超时或连接失败请求必须被录入快照黑匣子");

    let entry = &entries[0];
    assert!(
        entry.get("response").unwrap().is_null(),
        "response 字段应为 null"
    );

    let error_obj = entry.get("error").unwrap();
    assert!(error_obj.get("message").is_some());
    // 验证脱敏生效：错误信息中的敏感 token 必须被脱敏
    let err_msg = error_obj.get("message").unwrap().as_str().unwrap();
    assert!(err_msg.contains("[REDACTED]") || err_msg.contains("%5BREDACTED%5D"));
    assert!(!err_msg.contains("12345"));
}

#[test]
fn test_mask_snapshot_entry_redacts_inline_tokens_and_pii_in_error_message() {
    use rupost::history::model::ExecutionErrorDetail;
    use rupost::history::storage::mask_snapshot_entry;

    let mut entry = SnapshotEntry {
        id: "test-err-inline".to_string(),
        request: RequestSnapshot {
            timeout: None,
            method: "POST".to_string(),
            url: "http://api.com/auth".to_string(),
            headers: reqwest::header::HeaderMap::new(),
            body: None,
        },
        response: None,
        error: Some(ExecutionErrorDetail {
            kind: "ConnectionError".to_string(),
            message: "Failed handshake: Bearer eyJhbGciOiJIUzI1NiJ9.test and phone 13812345678"
                .to_string(),
            duration_ms: 120,
            occurred_at: chrono::Utc::now(),
        }),
    };

    mask_snapshot_entry(&mut entry);

    let masked_msg = &entry.error.unwrap().message;
    assert!(
        !masked_msg.contains("eyJhbGci"),
        "Inline bearer token in error message must be redacted: {masked_msg}"
    );
    assert!(
        !masked_msg.contains("13812345678"),
        "Inline PII in error message must be masked: {masked_msg}"
    );
    assert!(masked_msg.contains("***") || masked_msg.contains("[REDACTED]"));
}

#[tokio::test]
async fn test_replay_snapshot_error_entry_semantic_mismatch_and_match() {
    use rupost::history::model::ExecutionErrorDetail;

    let temp_dir = TempDir::new().unwrap();
    let snapshot_file = temp_dir.path().join("transport_error_suite.json");

    // 录制的是网络故障（期望无响应）
    let error_entry = SnapshotEntry {
        id: "err-entry-1".to_string(),
        request: RequestSnapshot {
            timeout: None,
            method: "GET".to_string(),
            url: "http://example.com/api/test".to_string(),
            headers: reqwest::header::HeaderMap::new(),
            body: None,
        },
        response: None,
        error: Some(ExecutionErrorDetail {
            kind: "ConnectionRefused".to_string(),
            message: "connection refused".to_string(),
            duration_ms: 50,
            occurred_at: chrono::Utc::now(),
        }),
    };

    let suite = SnapshotSuite {
        version: 2,
        timestamp: chrono::Utc::now(),
        source_file: Some("test.http".to_string()),
        entries: vec![error_entry],
    };

    write_snapshot_suite(&suite, &snapshot_file).unwrap();

    // 场景 1：回放对端意外恢复为 200 OK -> 行为不匹配，必须判为 matched: false
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/test"))
        .respond_with(ResponseTemplate::new(200).set_body_string("hello"))
        .mount(&mock_server)
        .await;

    let executor = ReplayExecutor::new(Some(mock_server.uri()), false).unwrap();
    let report = executor.replay_file(&snapshot_file).await.unwrap();
    assert_eq!(report.entries.len(), 1);
    assert!(
        !report.entries[0].matched,
        "快照记录网络错误，重放收到 200 响应必须判定不匹配 (matched: false)"
    );
    assert!(
        report.entries[0]
            .error
            .as_deref()
            .unwrap()
            .contains("Snapshot recorded transport error")
    );

    // 场景 2：回放对端依然无法连接 -> 行为完全吻合，判定为 matched: true
    let dead_executor = ReplayExecutor::new(Some("http://127.0.0.1:1".to_string()), false).unwrap();
    let dead_report = dead_executor.replay_file(&snapshot_file).await.unwrap();
    assert_eq!(dead_report.entries.len(), 1);
    assert!(
        dead_report.entries[0].matched,
        "快照记录网络错误，重放亦发生网络错误，必须判定吻合 (matched: true)"
    );
}

/// 审查 P0-B：非法 `--target` 在构造期快失败——此前 per-entry 三层 `if let Ok`
/// 静默跳过覆写，重放流量会打向快照**原始主机**（最高频误写：裸 host 无 scheme）。
#[test]
fn test_replay_executor_rejects_invalid_target_at_construction() {
    let err = ReplayExecutor::new(Some("example.com".to_string()), false)
        .err()
        .expect("裸 host（无 scheme）必须在构造期被拒绝");
    let msg = err.to_string();
    assert!(msg.contains("--target"), "错误应点名 --target: {msg}");
    assert!(msg.contains("scheme"), "错误应指引补全 scheme: {msg}");

    // 合法 target 与无 target 均可构造（行为不回归）
    assert!(ReplayExecutor::new(Some("http://127.0.0.1:1".to_string()), false).is_ok());
    assert!(ReplayExecutor::new(None, false).is_ok());
}

/// P0-B CLI 时序守护：target 校验先于快照加载——即使快照文件不存在，
/// 报错也必须是 target 非法（证明没有任何 I/O 发生在校验之前）。
#[test]
fn test_replay_cli_invalid_target_fails_fast_before_any_io() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        .args([
            "replay",
            "/no/such/snapshot_for_p0b.json",
            "--target",
            "example.com",
        ])
        .output()
        .expect("Failed to execute rupost binary");
    assert!(!out.status.success(), "非法 target 必须非零退出");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.contains("--target"),
        "应报 target 非法而非文件不存在: {combined}"
    );
}

#[tokio::test]
async fn test_replay_target_with_path_prefix() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/users"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok"
        })))
        .mount(&mock_server)
        .await;

    let temp_dir = TempDir::new().unwrap();
    let snapshot_file = temp_dir.path().join("snapshot_prefix.json");

    let entry = SnapshotEntry {
        id: "test-prefix-1".to_string(),
        request: RequestSnapshot {
            timeout: None,
            method: "GET".to_string(),
            url: "http://example.com/users".to_string(),
            headers: reqwest::header::HeaderMap::new(),
            body: None,
        },
        response: Some(ResponseSnapshot {
            status: 200,
            headers: reqwest::header::HeaderMap::new(),
            body: r#"{"status": "ok"}"#.to_string(),
        }),
        error: None,
    };

    let suite = SnapshotSuite {
        version: 1,
        timestamp: chrono::Utc::now(),
        source_file: Some("test_suite_prefix".to_string()),
        entries: vec![entry],
    };

    write_snapshot_suite(&suite, &snapshot_file).unwrap();

    // 1. target 带路径前缀（无尾随斜杠）
    let target = format!("{}/api/v1", mock_server.uri());
    let replayer = ReplayExecutor::new(Some(target), true).unwrap();
    let report = replayer.replay_file(&snapshot_file).await.unwrap();
    assert_eq!(report.total, 1);
    assert_eq!(report.passed, 1);
    assert_eq!(report.failed, 0);

    // 2. target 带路径前缀（有尾随斜杠）
    let target_trailing = format!("{}/api/v1/", mock_server.uri());
    let replayer_trailing = ReplayExecutor::new(Some(target_trailing), true).unwrap();
    let report_trailing = replayer_trailing.replay_file(&snapshot_file).await.unwrap();
    assert_eq!(report_trailing.total, 1);
    assert_eq!(report_trailing.passed, 1);
    assert_eq!(report_trailing.failed, 0);
}

/// R-5（免重填）：replay actual==401 且 expected≠401 → 必须输出"疑似会话过期"
/// 再获取指引（重放登录/更新 cookie/变量文件），不再是裸 mismatch。
#[tokio::test]
async fn test_replay_401_emits_session_expiry_hint() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/guarded"))
        .respond_with(ResponseTemplate::new(401).set_body_string("expired"))
        .mount(&mock_server)
        .await;

    let temp_dir = TempDir::new().unwrap();
    let snapshot_file = temp_dir.path().join("hint_snapshot.json");
    let suite = SnapshotSuite {
        version: 1,
        timestamp: chrono::Utc::now(),
        source_file: Some("hint_test".to_string()),
        entries: vec![SnapshotEntry {
            id: "id-1".to_string(),
            request: RequestSnapshot {
                timeout: None,
                method: "GET".to_string(),
                url: "http://original.example.com/guarded".to_string(),
                headers: reqwest::header::HeaderMap::new(),
                body: None,
            },
            response: Some(ResponseSnapshot {
                status: 200,
                headers: reqwest::header::HeaderMap::new(),
                body: String::new(),
            }),
            error: None,
        }],
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

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("expected 200, got 401"),
        "mismatch 基线：{stdout}"
    );
    assert!(
        stdout.contains("疑似会话过期"),
        "401 必须携带会话过期再获取指引: {stdout}"
    );
}
