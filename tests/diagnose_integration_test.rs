//! Integration tests for fine-grained network diagnostics (`rupost diagnose`).

use rupost::http::{diagnose_url, print_diagnose_report};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

/// R3 壳层：`run_diagnose` 的 `--report json` 分支——stdout 必须是合法 JSON 报告
/// （13 键精确 schema 由 tests/report_schema_test.rs 锁定，此处守护 CLI 壳层本身）
#[tokio::test]
async fn test_run_diagnose_cli_json_report_stdout() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("HELLO DIAGNOSE"))
        .mount(&mock_server)
        .await;

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        .args(["d", mock_server.uri().as_str(), "--report", "json"])
        .output()
        .expect("run rupost diagnose --report json");

    assert!(out.status.success(), "diagnose success must exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let value: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("json report stdout must be valid JSON");
    assert_eq!(value["is_https"], serde_json::json!(false));
    assert_eq!(value["http_status"], serde_json::json!(200));
}

/// R3 壳层：诊断失败（DNS 不可解析）→ stderr 错误 + `process::exit(1)` 路径
#[test]
fn test_run_diagnose_cli_error_exit_one() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        .args([
            "d",
            "http://rupost-diagnostics-nonexistent.invalid",
            "--report",
            "json",
        ])
        .output()
        .expect("run rupost diagnose against unresolvable host");

    assert_eq!(out.status.code(), Some(1), "diagnose failure must exit 1");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Error:"), "stderr carries error: {stderr}");
}

#[tokio::test]
async fn test_diagnose_local_http_success() {
    // 1. 启动 Wiremock 服务端
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("HELLO DIAGNOSE"))
        .mount(&mock_server)
        .await;

    // 2. 对本地 mock 进行连通性诊断
    let result = diagnose_url(&mock_server.uri()).await;
    assert!(result.is_ok(), "Local HTTP diagnostics should succeed");

    let report = result.unwrap();
    assert!(!report.is_https);
    assert!(
        !report.resolved_ips.is_empty(),
        "Should resolve localhost IP"
    );
    assert!(report.dns_lookup_duration.as_nanos() > 0);
    assert!(report.tcp_connect_duration.as_nanos() > 0);
    assert!(
        report.tls_handshake_duration.is_none(),
        "HTTP should have no TLS duration"
    );
    assert!(report.cert_info.is_none(), "HTTP should have no cert info");
    assert_eq!(report.http_status, Some(200));
    assert_eq!(report.http_version, Some("HTTP/1.1".to_string()));
    assert!(report.ttfb.is_some(), "Should record TTFB");

    // 3. 测试高亮输出打印是否正常工作且不 panic
    print_diagnose_report(&report);
}

#[tokio::test]
async fn test_diagnose_public_https_best_effort() {
    // 探测公网以验证完整的 HTTPS -> TLS 握手 -> 证书解析链路
    let target_url = "https://httpbingo.org/get";
    let result = diagnose_url(target_url).await;

    match result {
        Ok(report) => {
            println!("\n=== Public HTTPS Diagnostics Success ===");
            assert!(report.is_https);
            assert!(!report.resolved_ips.is_empty());
            assert!(report.dns_lookup_duration.as_nanos() > 0);
            assert!(report.tcp_connect_duration.as_nanos() > 0);
            assert!(
                report.tls_handshake_duration.is_some(),
                "HTTPS must have TLS duration"
            );
            assert!(report.cert_info.is_some(), "HTTPS must parse cert info");

            let cert = report.cert_info.as_ref().unwrap();
            assert!(!cert.subject.is_empty());
            assert!(!cert.issuer.is_empty());

            assert!(report.http_status.is_some());

            // 打印报告
            print_diagnose_report(&report);
        }
        Err(e) => {
            println!(
                "Skipping public HTTPS diagnostics integration test: {}. (Expected in offline/network-constrained environments)",
                e
            );
        }
    }
}

#[tokio::test]
async fn test_diagnose_websocket_upgrade_success() {
    // 1. 启动 Mock 服务端返回 101 WebSocket Upgrade 响应
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(101)
                .insert_header("Upgrade", "websocket")
                .insert_header("Connection", "Upgrade"),
        )
        .mount(&mock_server)
        .await;

    // 2. 将 http 协议地址改写为 ws 协议地址，测试诊断
    let ws_url = mock_server.uri().replace("http://", "ws://");
    let result = diagnose_url(&ws_url).await;
    assert!(result.is_ok(), "WS diagnostics should succeed");

    let report = result.unwrap();
    assert!(!report.is_https);
    assert!(report.is_websocket);
    assert_eq!(report.ws_upgrade_success, Some(true));
    assert_eq!(report.http_status, Some(101));

    // 打印报告确保不 panic
    print_diagnose_report(&report);
}
