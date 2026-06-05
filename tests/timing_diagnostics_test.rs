//! Tests for fine-grained network diagnostics timing.

use rupost::http::timing::DiagnosticsProber;

#[tokio::test]
async fn test_diagnostics_probe_localhost() {
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // 启动 Mock 服务端
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("OK"))
        .mount(&mock_server)
        .await;

    // 测量本地 Mock 服务器连接
    let probe_res = DiagnosticsProber::probe_connection(&mock_server.uri()).await;

    assert!(
        probe_res.is_ok(),
        "Probe should succeed on active mock server"
    );
    let (dns, tcp) = probe_res.unwrap();

    println!("\n=== Localhost Connection Timing Probe ===");
    println!("DNS Lookup  : {} ms", dns.as_millis());
    println!("TCP Connect : {} ms", tcp.as_millis());

    assert!(
        tcp.as_millis() < 500,
        "TCP connect to localhost should be very fast"
    );
}

#[tokio::test]
async fn test_diagnostics_probe_public_url() {
    // 探测公网公共接口以验证 DNS 和 TCP
    let probe_res = DiagnosticsProber::probe_connection("https://httpbingo.org/get").await;

    // 如果有公网连接则测试通过
    if let Ok((dns, tcp)) = probe_res {
        println!("\n=== Public URL Connection Timing Probe ===");
        println!("DNS Lookup  : {} ms", dns.as_millis());
        println!("TCP Connect : {} ms", tcp.as_millis());
        assert!(dns.as_nanos() > 0);
        assert!(tcp.as_nanos() > 0);
    } else {
        println!(
            "Skipping public DNS/TCP verification due to offline/network environment constraints."
        );
    }
}
