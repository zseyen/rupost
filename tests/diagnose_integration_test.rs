//! Integration tests for fine-grained network diagnostics (`rupost diagnose`).

use rupost::http::{diagnose_url, diagnose_url_lenient, print_diagnose_report};
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

    // 大小写统一守护（backlog 收尾）：--report JSON 大写同语义
    let out_upper = std::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        .args(["d", mock_server.uri().as_str(), "--report", "JSON"])
        .output()
        .expect("run rupost diagnose --report JSON");
    assert!(out_upper.status.success(), "diagnose success must exit 0");
    let stdout_upper = String::from_utf8_lossy(&out_upper.stdout);
    let value_upper: serde_json::Value =
        serde_json::from_str(stdout_upper.trim()).expect("uppercase JSON must be machine-readable");
    assert!(value_upper.is_object());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let value: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("json report stdout must be valid JSON");
    assert_eq!(value["is_https"], serde_json::json!(false));
    assert_eq!(value["http_status"], serde_json::json!(200));
}

/// R3 壳层：诊断失败（TCP 连接拒绝，回环确定性触发）→ stderr 错误 + `process::exit(1)` 路径。
/// 弃用 .invalid 域名：挟持网络（VPN fake-IP 通配解析）下会假绿（对抗审查预告的风险成真）。
///
/// 环境哨兵（2026-11-05 勘误）：透明代理/增强模式代理可截胡**任意端口**的出站
/// 连接——包括回环——使 127.0.0.1:1 "可连"，诊断以代理错误响应正常完成
/// （exit 0），契约断言在该环境下不再确定性成立。测试前先裸 connect 探测：
/// 可连则跳过（环境敌意，非产品缺陷）；拒绝则按原契约锁定。
#[test]
fn test_run_diagnose_cli_error_exit_one() {
    if std::net::TcpStream::connect("127.0.0.1:1").is_ok() {
        eprintln!(
            "SKIP: 127.0.0.1:1 is connectable in this environment \
             (transparent proxy interception) — exit-code contract is not \
             deterministically testable here; re-run on a clean network"
        );
        return;
    }

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rupost"))
        .args(["d", "http://127.0.0.1:1", "--report", "json"])
        .output()
        .expect("run rupost diagnose against closed loopback port");

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

#[tokio::test]
async fn test_diagnose_lenient_preserves_partial_stages_on_http_failure() {
    // 启动监听端口，accept 连接后立即挂断，触发 HTTP 读写阶段失败
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{}", port);

    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            // 立即丢弃 stream，对端在尝试读取 HTTP 响应时将遭遇 EOF
            drop(stream);
        }
    });

    // 1. 严格契约：diagnose_url 必须返回 Err
    let strict_res = diagnose_url(&url).await;
    assert!(
        strict_res.is_err(),
        "严格诊断在 HTTP 阶段失败时必须返回 Err"
    );

    // 2. 宽容契约：diagnose_url_lenient 必须返回 Ok，且保留前期阶段数据
    let lenient_res = diagnose_url_lenient(&url).await;
    assert!(
        lenient_res.is_ok(),
        "宽容诊断在 HTTP 阶段失败时必须返回 Ok 降级报告"
    );
    let report = lenient_res.unwrap();

    assert!(
        report.connection_error.is_some(),
        "connection_error 必须记录 HTTP 阶段失败原因"
    );
    assert!(
        report.tcp_connect_duration.as_nanos() > 0,
        "必须保留已成功的 TCP 握手耗时"
    );
    assert!(
        report.dns_lookup_duration.as_nanos() > 0,
        "必须保留已成功的 DNS 解析耗时"
    );
    assert_eq!(report.http_status, None, "未完成的 HTTP 状态码必须为 None");
}
