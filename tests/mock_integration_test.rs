use rupost::mock::matcher::TrieRouteMatcher;
use rupost::mock::server::{AxumMockServer, MockServer};
use rupost::mock::variant::{CompareOp, ConditionSource, MockVariant, VariantCondition};
use std::collections::HashMap;
use std::sync::Arc;

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
    matcher.add_route(
        "POST",
        "/api/pay",
        vec![admin_pay_variant, guest_pay_variant],
    );

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
    let resp = client
        .get(format!("http://127.0.0.1:{}/api/users/999", port))
        .send()
        .await
        .expect("Failed to connect to mock server");

    assert_eq!(resp.status().as_u16(), 200);
    let body = resp.text().await.unwrap();
    assert!(
        body.contains(r#""user_id": "999""#),
        "Expected interpolated user_id 999, got: {}",
        body
    );

    // 发送 POST 满足 admin-key 条件的请求
    let resp_admin = client
        .post(format!("http://127.0.0.1:{}/api/pay", port))
        .header("Authorization", "admin-key")
        .send()
        .await
        .unwrap();
    assert_eq!(resp_admin.status().as_u16(), 200);
    let body_admin = resp_admin.text().await.unwrap();
    assert!(body_admin.contains("admin payment processed"));

    // 发送 POST 满足 guest-key 条件的请求
    let resp_guest = client
        .post(format!("http://127.0.0.1:{}/api/pay", port))
        .header("Authorization", "guest-key")
        .send()
        .await
        .unwrap();
    assert_eq!(resp_guest.status().as_u16(), 403);
    let body_guest = resp_guest.text().await.unwrap();
    assert!(body_guest.contains("forbidden for guest"));
}

#[tokio::test]
async fn test_markdown_api_lifecycle_smoke_tests() {
    use rupost::parser::{MarkdownFileParser, MockCompiler};
    use rupost::runner::{DirectoryScanner, WorkflowGraph};

    // 1. 测试用例 1-3: 加载 v2_api_evolution.md，验证 V1 兼容、V2 正常、V2 拦截
    let evol_file = "examples/iteration_scenarios/v2_api_evolution.md";
    let parsed_evol = MarkdownFileParser::parse_file(evol_file).unwrap();
    let routes_evol = MockCompiler::compile(&parsed_evol);

    let mut matcher = TrieRouteMatcher::new();
    for r in routes_evol {
        matcher.add_route(&r.method, &r.path, r.variants);
    }

    let matcher = Arc::new(matcher);
    let port = 19091;

    let server = AxumMockServer;
    tokio::spawn(async move {
        let _ = server.start(port, matcher).await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let client = reqwest::Client::new();

    // 冒烟点 1: 验证 V1 兼容接口返回了 "compat_mode": true
    let resp_v1 = client
        .get(format!("http://127.0.0.1:{}/api/v1/orders/101?status=completed", port))
        .send()
        .await
        .unwrap();
    assert_eq!(resp_v1.status().as_u16(), 200);
    let body_v1 = resp_v1.text().await.unwrap();
    assert!(body_v1.contains(r#""compat_mode": true"#));
    assert!(body_v1.contains(r#""payment_method": "legacy""#));

    // 冒烟点 2: 验证 V2 写入操作在未携带 X-App-Version 时被默认拦截 (返回 400)
    let resp_v2_fail = client
        .post(format!("http://127.0.0.1:{}/api/v2/orders", port))
        .send()
        .await
        .unwrap();
    assert_eq!(resp_v2_fail.status().as_u16(), 400);
    assert!(resp_v2_fail.text().await.unwrap().contains("Upgrade required"));

    // 冒烟点 3: 验证 V2 携带 X-App-Version == 2.0.0 正常放行通过 (返回 201)
    let resp_v2_ok = client
        .post(format!("http://127.0.0.1:{}/api/v2/orders", port))
        .header("X-App-Version", "2.0.0")
        .send()
        .await
        .unwrap();
    assert_eq!(resp_v2_ok.status().as_u16(), 201);
    assert!(resp_v2_ok.text().await.unwrap().contains(r#""x_app_validated": true"#));

    // 2. 测试用例 4: 网关安全与限流 Header-None 判定冒烟
    let sec_file = "examples/design_scenarios/security_and_ratelimit.md";
    let parsed_sec = MarkdownFileParser::parse_file(sec_file).unwrap();
    let routes_sec = MockCompiler::compile(&parsed_sec);

    let mut matcher_sec = TrieRouteMatcher::new();
    for r in routes_sec {
        matcher_sec.add_route(&r.method, &r.path, r.variants);
    }

    let matcher_sec = Arc::new(matcher_sec);
    let port_sec = 19092;

    let server_sec = AxumMockServer;
    tokio::spawn(async move {
        let _ = server_sec.start(port_sec, matcher_sec).await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // 冒烟点 4: 不带 Authorization 时匹配 header.Authorization == None 从而返回 401
    let resp_sec_401 = client
        .post(format!("http://127.0.0.1:{}/api/v1/accounts/reset-password", port_sec))
        .send()
        .await
        .unwrap();
    assert_eq!(resp_sec_401.status().as_u16(), 401);
    assert!(resp_sec_401.text().await.unwrap().contains("Missing Authorization"));

    // 冒烟点 5: 带过期 Token 触发 403
    let resp_sec_403 = client
        .post(format!("http://127.0.0.1:{}/api/v1/accounts/reset-password", port_sec))
        .header("Authorization", "Bearer expired_token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp_sec_403.status().as_u16(), 403);
    assert!(resp_sec_403.text().await.unwrap().contains("expired"));

    // 3. 测试用例 5: 级联多文件依赖 @depends-on 合并加载冒烟
    let test_file = "examples/iteration_scenarios/migration_test.md";
    
    let scanned_files = DirectoryScanner::scan(&[test_file.to_string()]).unwrap();
    let mut files_map = std::collections::HashMap::new();
    let mut parse_pairs = Vec::new();
    for p in scanned_files {
        let parsed = if p.extension().and_then(|s| s.to_str()) == Some("md") {
            MarkdownFileParser::parse_file(&p).unwrap()
        } else {
            rupost::parser::HttpFileParser::parse_file(&p).unwrap()
        };
        files_map.insert(p.clone(), parsed.clone());
        parse_pairs.push((p, parsed));
    }

    let graph = WorkflowGraph::new(&parse_pairs);
    let execution_order = graph.resolve_execution_order().unwrap();

    let mut all_routes = Vec::new();
    for p in execution_order {
        if let Some(parsed) = files_map.get(&p) {
            let routes = MockCompiler::compile(parsed);
            all_routes.extend(routes);
        }
    }

    let mut matcher_dep = TrieRouteMatcher::new();
    for r in all_routes {
        matcher_dep.add_route(&r.method, &r.path, r.variants);
    }

    let matcher_dep = Arc::new(matcher_dep);
    let port_dep = 19093;

    let server_dep = AxumMockServer;
    tokio::spawn(async move {
        let _ = server_dep.start(port_dep, matcher_dep).await;
    });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // 冒烟点 6: 从联合加载的依赖路由树中，请求来自于 v1_api.md 的接口
    let resp_dep_v1 = client
        .get(format!("http://127.0.0.1:{}/api/v1/orders/777?status=completed", port_dep))
        .send()
        .await
        .unwrap();
    assert_eq!(resp_dep_v1.status().as_u16(), 200);
    assert!(resp_dep_v1.text().await.unwrap().contains(r#""status": "completed""#));

    // 冒烟点 7: 从联合加载的依赖路由树中，请求来自于 v2_api_evolution.md 的新 V2 接口
    let resp_dep_v2 = client
        .post(format!("http://127.0.0.1:{}/api/v2/orders", port_dep))
        .header("X-App-Version", "2.0.0")
        .send()
        .await
        .unwrap();
    assert_eq!(resp_dep_v2.status().as_u16(), 201);
    assert!(resp_dep_v2.text().await.unwrap().contains(r#""x_app_validated": true"#));
}
