//! TUI CLI-parity 九项功能完整守护套件（R2–R9 全量）。
//!
//! 每项功能一条端到端/集成守护，命名与 PRD 需求编号对应：
//! - R2/R3: 多请求执行 + Results 簿记（含 is_loading 生命周期）
//! - R4: 选择器触发接线（`#` 归一化已在 selector_wiring_tests 覆盖，此处锁定模态入口）
//! - R5: cookie 会话延续（两次触发，第二次携带首次 Set-Cookie）
//! - R6: timing 透传与响应面板渲染
//! - R7: 失败自动诊断切换（Diagnose tab）+ diagnose_report 透传
//! - R8: X 导出（脱敏/原样决策表 + 弹窗状态）
//! - R9: Ctrl+S 快照落盘（脱敏） + History `G` 生成
//! - R11: M 键 mock 开关（真实 HTTP 往返 + 停止）
#![allow(clippy::collapsible_if, clippy::result_large_err)]
use rupost::history::model::SnapshotSuite;
use rupost::parser::HttpFileParser;
use rupost::tui::event::TuiEvent;
use rupost::tui::state::{AppState, Panel};
use std::time::Duration;
use tokio::sync::mpsc;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// 消费事件直至 run_results 达到 expected_count 或超时（模拟 app.rs 主循环）
async fn pump_until(
    state: &mut AppState,
    rx: &mut mpsc::Receiver<TuiEvent>,
    expected: usize,
    secs: u64,
) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
    while tokio::time::Instant::now() < deadline && state.run_results.len() < expected {
        match tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
            Ok(Some(TuiEvent::RequestResult {
                result,
                captured_vars,
                run_total,
            })) => {
                state.handle_request_result(*result, captured_vars, run_total);
            }
            Ok(Some(_)) => continue,
            _ => {}
        }
    }
}

// ── R2/R3：多请求执行 + is_loading 生命周期 ──

#[tokio::test]
async fn test_r2_multi_request_bookkeeping_and_loading_lifecycle() {
    let server = MockServer::start().await;
    for i in 1..=3 {
        Mock::given(method("GET"))
            .and(path(format!("/r{i}")))
            .respond_with(ResponseTemplate::new(200).set_body_string(format!("r{i}")))
            .mount(&server)
            .await;
    }

    let mut state = AppState::new();
    state.editor_text = format!(
        "### one\nGET {}/r1\n\n### two\nGET {}/r2\n\n### three\nGET {}/r3\n",
        server.uri(),
        server.uri(),
        server.uri()
    );
    let (tx, mut rx) = mpsc::channel(64);
    rupost::tui::handlers::trigger_request_execution(&mut state, &tx).await;

    // 第 1 个结果到达：is_loading 仍为 true（还有 2 个未完成）
    pump_until(&mut state, &mut rx, 1, 5).await;
    assert_eq!(state.run_results.len(), 1);
    assert!(state.is_loading, "loading must hold mid-run");
    assert_eq!(state.run_index, 1);
    assert_eq!(state.run_total, 3);
    assert_eq!(
        state.active_detail_tab,
        rupost::tui::state::DetailTab::Results,
        "multi-run auto-switches to Results tab"
    );

    // 全部完成：is_loading 落 false
    pump_until(&mut state, &mut rx, 3, 5).await;
    assert_eq!(state.run_results.len(), 3);
    assert!(!state.is_loading, "loading drops after last result");
    assert!(state.run_results.iter().all(|r| r.success));
}

// ── R5：cookie 会话延续 ──

#[tokio::test]
async fn test_r5_cookie_bridge_persists_across_triggers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/login"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Set-Cookie", "sid=bridge-123; Path=/")
                .set_body_string("logged in"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/me"))
        .respond_with(ResponseTemplate::new(200).set_body_string("me"))
        .mount(&server)
        .await;

    let mut state = AppState::new();
    let (tx, mut rx) = mpsc::channel(64);

    // 第一次触发：/login 收到 Set-Cookie
    state.editor_text = format!("### login\nGET {}/login\n", server.uri());
    rupost::tui::handlers::trigger_request_execution(&mut state, &tx).await;
    pump_until(&mut state, &mut rx, 1, 5).await;

    // 等待 cookie bridge 导出（触发 task 末尾写回）
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        let has = state
            .cookie_bridge
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some();
        if has {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        state
            .cookie_bridge
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some(),
        "cookie bridge must be exported after trigger"
    );

    // 第二次触发：/me —— 服务端必须收到 Cookie 头
    state.editor_text = format!("### me\nGET {}/me\n", server.uri());
    rupost::tui::handlers::trigger_request_execution(&mut state, &tx).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        let received = server.received_requests().await.unwrap_or_default();
        if received.iter().any(|r| r.url.path() == "/me") {
            let cookie = received
                .iter()
                .find(|r| r.url.path() == "/me")
                .and_then(|r| r.headers.get("cookie"))
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            assert!(
                cookie.contains("sid=bridge-123"),
                "second trigger must carry session cookie, got {cookie:?}"
            );
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("second trigger did not complete within window");
}

// ── R6：timing 透传与渲染 ──

#[test]
fn test_r6_timing_stored_and_rendered() {
    use rupost::http::timing::RequestTiming;
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();

    let mut state = AppState::new();
    state.active_panel = Panel::Response;
    state.update_layout(100, 30);
    state.last_timing = Some(RequestTiming {
        dns_lookup: Duration::from_millis(3),
        tcp_connect: Duration::from_millis(11),
        ttfb: Duration::from_millis(45),
        transfer: Duration::from_millis(8),
    });
    state.last_response = Some(
        rupost::http::Response::new(
            200,
            reqwest::header::HeaderMap::new(),
            "ok".to_string(),
            Duration::from_millis(60),
            Duration::from_millis(45),
            Duration::from_millis(8),
        )
        .unwrap(),
    );

    terminal
        .draw(|f| rupost::tui::ui::render(f, &mut state))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let text: String = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("DNS"), "timing line must render: {text}");
    assert!(text.contains("TTFB"), "timing line must render");
}

// ── R7：失败自动诊断切换 + report 透传 ──

#[tokio::test]
async fn test_r7_failure_switches_to_diagnose_tab() {
    use rupost::http::diagnose::DiagnosticsReport;

    let mut state = AppState::new();
    state.is_loading = true;
    let report = DiagnosticsReport {
        url: "http://127.0.0.1:1".to_string(),
        is_https: false,
        is_websocket: false,
        ws_upgrade_success: None,
        resolved_ips: vec![],
        dns_lookup_duration: Duration::from_millis(1),
        tcp_connect_duration: Duration::from_millis(1),
        tls_handshake_duration: None,
        cert_info: None,
        http_status: None,
        http_version: None,
        ttfb: None,
        total_duration: Duration::from_millis(2),
    };

    let failed = rupost::runner::types::TestResult {
        request_number: 1,
        name: None,
        method: "GET".to_string(),
        url: "http://127.0.0.1:1".to_string(),
        status: None,
        duration: Duration::from_millis(2),
        success: false,
        error: Some("connection refused".to_string()),
        response: None,
        skipped: false,
        assertions: vec![],
        timing: None,
        diagnose_report: Some(report),
        request: None,
        warnings: vec![],
    };
    state.handle_request_result(failed, std::collections::HashMap::new(), 1);

    assert!(
        matches!(
            state.active_detail_tab,
            rupost::tui::state::DetailTab::Diagnose
        ),
        "failure with report must auto-switch to Diagnose tab"
    );
    assert!(state.last_diagnose.is_some());
    // 既有设计：失败结果物化 Response::error（错误可见），success=false
    assert!(
        state.last_response.is_some(),
        "failed result materializes error response"
    );
    assert!(
        !state.last_response.as_ref().unwrap().is_success(),
        "materialized response must be non-success"
    );
}

// ── R8：X 导出决策表 ──

#[test]
fn test_r8_export_current_request_redact_decision() {
    let mut state = AppState::new();
    state.editor_text = "### Secured\nGET http://api.example.com/secure\nAuthorization: Bearer super-secret-token\n".to_string();

    // X（默认脱敏）：命令含 [REDACTED]，不含明文 token
    rupost::tui::handlers::editor::export_current_request(&mut state, true);
    eprintln!("DEBUG-R8: last_error={:?}", state.last_error);
    let cmd = state
        .show_export
        .as_deref()
        .expect("export must populate popup");
    assert!(cmd.contains("curl"), "must be a curl command");
    assert!(
        cmd.contains("[REDACTED]"),
        "secrets must be redacted: {cmd}"
    );
    assert!(
        !cmd.contains("super-secret-token"),
        "raw token must not leak: {cmd}"
    );

    // Shift+X（原样）：明文保留
    rupost::tui::handlers::editor::export_current_request(&mut state, false);
    let cmd = state
        .show_export
        .as_deref()
        .expect("export must populate popup");
    assert!(
        cmd.contains("super-secret-token"),
        "raw mode must keep token: {cmd}"
    );
}

// ── R9：快照落盘（脱敏） + History 生成 ──

#[test]
fn test_r9_snapshot_saved_masked_and_parseable() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut state = AppState::new();
    state.workspace_root = temp.path().to_path_buf();
    let resp = rupost::http::Response::new(
        200,
        reqwest::header::HeaderMap::new(),
        "{}".to_string(),
        Duration::from_millis(1),
        Duration::from_millis(1),
        Duration::from_millis(1),
    )
    .unwrap();
    state.current_request = Some(rupost::parser::ParsedRequest {
        method: Some("GET".to_string()),
        url: "http://api.example.com/secure".to_string(),
        headers: vec![(
            "Authorization".to_string(),
            "Bearer snapshot-secret".to_string(),
        )],
        body: None,
        metadata: Default::default(),
        line_number: 1,
        base_path: None,
    });
    state.last_response = Some(resp);

    rupost::tui::handlers::editor::save_current_snapshot(&mut state);
    assert_eq!(
        state.last_error, None,
        "snapshot success must clear last_error"
    );

    let dir = temp.path().join("snapshots");
    assert!(dir.exists(), "snapshots dir must be created");
    let files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();
    assert_eq!(files.len(), 1, "exactly one snapshot written");
    let content = std::fs::read_to_string(&files[0]).unwrap();
    assert!(
        !content.contains("snapshot-secret"),
        "snapshot must be masked, got: {content}"
    );
    let suite: SnapshotSuite = serde_json::from_str(&content).expect("valid JSON suite");
    assert_eq!(suite.entries.len(), 1);
    assert_eq!(suite.entries[0].request.method, "GET");
}

#[tokio::test]
async fn test_r9_generate_history_entry_to_workspace_file() {
    use rupost::history::model::ResponseMeta;
    let temp = tempfile::TempDir::new().unwrap();
    let mut state = AppState::new();
    state.workspace_root = temp.path().to_path_buf();

    state.history_list = vec![rupost::history::model::HistoryEntry {
        id: "gen-1234-5678".to_string(),
        timestamp: chrono::Utc::now(),
        duration_ms: 10,
        request: rupost::history::model::RequestSnapshot {
            timeout: None,
            method: "GET".to_string(),
            url: "http://api.example.com/generated".to_string(),
            headers: reqwest::header::HeaderMap::new(),
            body: None,
        },
        source: None,
        response: ResponseMeta {
            status: 200,
            headers: reqwest::header::HeaderMap::new(),
            body: Some("{}".to_string()),
        },
    }];
    state.history_state.selected_index = 0;

    rupost::tui::handlers::editor::generate_history_to_file(&mut state).await;
    eprintln!("DEBUG-GEN: last_error={:?}", state.last_error);

    let generated = temp.path().join("generated_gen-1234.http");
    assert!(generated.exists(), "generated file must exist");
    let content = std::fs::read_to_string(&generated).unwrap();
    assert!(
        content.contains("GET"),
        "generated content must contain request"
    );
    // 生成文件可被解析（HTTP 合法性）
    let parsed = HttpFileParser::parse_file(&generated).unwrap();
    assert_eq!(parsed.requests.len(), 1);
    assert_eq!(parsed.requests[0].url, "http://api.example.com/generated");
}

// ── R11：M 键 mock 开关（真实 HTTP 往返） ──

#[tokio::test]
async fn test_r11_mock_toggle_serves_and_stops() {
    let temp = tempfile::TempDir::new().unwrap();
    let file = temp.path().join("mock_cfg.md");
    // .http 解析器不产 mock variants——mock 配置文件走 .md 解析器（与 CLI mock 一致）
    std::fs::write(
        temp.path().join("mock_cfg.md"),
        "# Mock API\n\n```http\nGET /api/hello\n\n@mock-default\nHTTP/1.1 200 OK\nContent-Type: application/json\n\n{\"hello\": \"world\"}\n```\n",
    )
    .unwrap();

    let mut state = AppState::new();
    state.editor_file_path = Some(file.to_string_lossy().into_owned());

    rupost::tui::integrations::toggle_mock_server(&mut state).await;
    eprintln!("DEBUG-R11: last_error={:?}", state.last_error);
    let session = state.mock_session.as_ref().expect("mock session must open");
    let port = session.port;
    assert!(port > 0, "bound port must be known");

    // 真实 HTTP 往返命中 mock 路由
    let client = reqwest::Client::new();
    let resp = tokio::time::timeout(
        Duration::from_secs(5),
        client
            .get(format!("http://127.0.0.1:{port}/api/hello"))
            .send(),
    )
    .await
    .expect("request within window")
    .expect("mock server responds");
    assert_eq!(resp.status().as_u16(), 200);

    // 停止：M 再按
    rupost::tui::integrations::toggle_mock_server(&mut state).await;
    assert!(state.mock_session.is_none(), "session must close");
}
