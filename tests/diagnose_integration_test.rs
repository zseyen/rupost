//! Integration tests for fine-grained network diagnostics (`rupost diagnose`).

use rupost::http::{diagnose_url, print_diagnose_report};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

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
