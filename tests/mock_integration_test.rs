use std::collections::HashMap;
use std::sync::Arc;
use rupost::mock::matcher::TrieRouteMatcher;
use rupost::mock::server::{MockServer, AxumMockServer};
use rupost::mock::variant::{MockVariant, VariantCondition, ConditionSource, CompareOp};

#[tokio::test]
async fn test_mock_server_integration_flow() {
    let mut matcher = TrieRouteMatcher::new();

    // 1. 添加带路径变量的路由: /api/users/:id
    let user_variant = MockVariant {
        condition: None,
        status: 200,
        headers: {
            let mut h = HashMap::new();
            h.insert("Content-Type".to_string(), "application/json".to_string());
            h
        },
        response_body: r#"{"user_id": "{{id}}", "status": "active"}"#.to_string(),
    };
    matcher.add_route("GET", "/api/users/:id", vec![user_variant]);

    // 2. 添加带条件分支的路由: /api/pay
    let admin_pay_variant = MockVariant {
        condition: Some(VariantCondition {
            source: ConditionSource::Header,
            key: "Authorization".to_string(),
            operator: CompareOp::Equals,
            expected_value: "admin-key".to_string(),
        }),
        status: 200,
        headers: HashMap::new(),
        response_body: r#"{"message": "admin payment processed"}"#.to_string(),
    };
    let guest_pay_variant = MockVariant {
        condition: Some(VariantCondition {
            source: ConditionSource::Header,
            key: "Authorization".to_string(),
            operator: CompareOp::Equals,
            expected_value: "guest-key".to_string(),
        }),
        status: 403,
        headers: HashMap::new(),
        response_body: r#"{"error": "forbidden for guest"}"#.to_string(),
    };
    matcher.add_route("POST", "/api/pay", vec![admin_pay_variant, guest_pay_variant]);

    let matcher = Arc::new(matcher);
    
    // 使用随机可用端口 (在此硬编码一个本地动态端口)
    let port = 19090;
    
    // 启动 AxumMockServer
    let server = AxumMockServer;
    tokio::spawn(async move {
        let _ = server.start(port, matcher).await;
    });
    
    // 等待一小会儿让服务器启动
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // 发送请求，此处应能成功连上并返回预期的匹配内容
    let client = reqwest::Client::new();
    let resp = client.get(format!("http://127.0.0.1:{}/api/users/999", port))
        .send()
        .await
        .expect("Failed to connect to mock server");
        
    assert_eq!(resp.status().as_u16(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains(r#""user_id": "999""#), "Expected interpolated user_id 999, got: {}", body);

    // 发送 POST 满足 admin-key 条件的请求
    let resp_admin = client.post(format!("http://127.0.0.1:{}/api/pay", port))
        .header("Authorization", "admin-key")
        .send()
        .await
        .unwrap();
    assert_eq!(resp_admin.status().as_u16(), 200);
    let body_admin = resp_admin.text().await.unwrap();
    assert!(body_admin.contains("admin payment processed"));

    // 发送 POST 满足 guest-key 条件的请求
    let resp_guest = client.post(format!("http://127.0.0.1:{}/api/pay", port))
        .header("Authorization", "guest-key")
        .send()
        .await
        .unwrap();
    assert_eq!(resp_guest.status().as_u16(), 403);
    let body_guest = resp_guest.text().await.unwrap();
    assert!(body_guest.contains("forbidden for guest"));
}
