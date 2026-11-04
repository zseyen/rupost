// 进程级静音历史落盘
mod common;

use rupost::mock::compiler::MockCompiler;
use rupost::mock::matcher::TrieRouteMatcher;
use rupost::mock::server::{AxumMockServer, MockServer};
use rupost::parser::{HttpFileParser, MarkdownFileParser};
use std::path::PathBuf;
use std::sync::Arc;

#[test]
fn test_scenario_order_fulfillment_spec_structure() {
    let spec_path = PathBuf::from("examples/scenario_order_fulfillment/order_spec.md");
    let parsed_spec =
        MarkdownFileParser::parse_file(&spec_path).expect("order_spec.md should be parsed cleanly");

    // 1. 验证 Frontmatter 元数据
    assert!(
        parsed_spec
            .metadata
            .title
            .as_deref()
            .unwrap_or("")
            .contains("电商交易与履约服务规格书")
    );
    assert_eq!(parsed_spec.metadata.version.as_deref(), Some("1.0.0"));

    // 2. 验证包含 2 个核心代码块
    assert_eq!(
        parsed_spec.requests.len(),
        2,
        "应包含创建订单与订单详情 2 个契约块"
    );

    // 块 1: 创建订单（单块双角色）
    let create_order = &parsed_spec.requests[0];
    assert_eq!(
        create_order.metadata.name.as_deref(),
        Some("create-order-standard")
    );
    assert_eq!(create_order.method_or_default(), "POST");
    assert_eq!(create_order.url, "/api/v1/orders");
    assert!(
        create_order.metadata.is_test,
        "主块带有 @test，作为冒烟用例"
    );
    assert_eq!(
        create_order.metadata.mock_variants.len(),
        4,
        "应解析出 4 个 Mock 变体分支"
    );

    // 块 2: 订单详情（纯契约定义）
    let get_detail = &parsed_spec.requests[1];
    assert_eq!(
        get_detail.metadata.name.as_deref(),
        Some("get-order-detail")
    );
    assert_eq!(get_detail.method_or_default(), "GET");
    assert_eq!(get_detail.url, "/api/v1/orders/detail");
    assert!(
        !get_detail.metadata.is_test,
        "纯契约块无 @test，测试期必须跳过"
    );
    assert_eq!(
        get_detail.metadata.mock_variants.len(),
        2,
        "应解析出 404 与兜底 200 变体"
    );

    // 3. 验证执行态测试套件 .http 结构
    let test_suite_path =
        PathBuf::from("examples/scenario_order_fulfillment/order_test_suite.http");
    let parsed_suite = HttpFileParser::parse_file(&test_suite_path)
        .expect("order_test_suite.http should be parsed cleanly");
    assert_eq!(
        parsed_suite.requests.len(),
        7,
        "测试套件应包含 7 个端到端回归用例"
    );
}

#[test]
fn test_scenario_iam_tenant_spec_structure() {
    let spec_path = PathBuf::from("examples/scenario_iam_tenant/iam_spec.md");
    let parsed_spec =
        MarkdownFileParser::parse_file(&spec_path).expect("iam_spec.md should be parsed cleanly");

    // 1. 验证 Frontmatter 元数据
    assert!(
        parsed_spec
            .metadata
            .title
            .as_deref()
            .unwrap_or("")
            .contains("多租户企业级权限")
    );
    assert_eq!(parsed_spec.metadata.version.as_deref(), Some("1.0.0"));

    // 2. 验证代码块结构
    assert_eq!(
        parsed_spec.requests.len(),
        2,
        "应包含角色查询与创建成员 2 个契约块"
    );

    // 块 1: 角色权限查询（单块双角色）
    let query_roles = &parsed_spec.requests[0];
    assert_eq!(
        query_roles.metadata.name.as_deref(),
        Some("query-tenant-roles")
    );
    assert_eq!(query_roles.method_or_default(), "GET");
    assert_eq!(query_roles.url, "/api/v1/tenants/roles");
    assert!(query_roles.metadata.is_test, "带有 @test 标记");
    assert_eq!(
        query_roles.metadata.mock_variants.len(),
        4,
        "包含 403、admin、member、兜底 4 个分支"
    );

    // 块 2: 租户用户创建（纯契约定义）
    let create_user = &parsed_spec.requests[1];
    assert_eq!(
        create_user.metadata.name.as_deref(),
        Some("create-tenant-user")
    );
    assert_eq!(create_user.method_or_default(), "POST");
    assert_eq!(create_user.url, "/api/v1/tenants/users");
    assert!(!create_user.metadata.is_test, "纯契约块无 @test");
    assert_eq!(
        create_user.metadata.mock_variants.len(),
        3,
        "包含 400、409、兜底 201 3 个分支"
    );

    // 3. 验证执行态测试套件 .http 结构
    let test_suite_path = PathBuf::from("examples/scenario_iam_tenant/iam_test_suite.http");
    let parsed_suite = HttpFileParser::parse_file(&test_suite_path)
        .expect("iam_test_suite.http should be parsed cleanly");
    assert_eq!(
        parsed_suite.requests.len(),
        7,
        "测试套件应包含 7 个权限回归用例"
    );
}

#[tokio::test]
async fn test_scenario_order_fulfillment_mock_server_flow() {
    let spec_path = PathBuf::from("examples/scenario_order_fulfillment/order_spec.md");
    let parsed_spec = MarkdownFileParser::parse_file(&spec_path).unwrap();

    // 编译纯契约路由
    let routes = MockCompiler::compile(&parsed_spec);
    let mut matcher = TrieRouteMatcher::new();
    for r in routes {
        matcher.add_route(&r.method, &r.path, r.variants);
    }
    let matcher = Arc::new(matcher);
    let port = 19188;

    let server = AxumMockServer;
    tokio::spawn(async move {
        let _ = server.start(port, matcher).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let client = reqwest::Client::new();

    // 1. 测试纯契约块 GET /api/v1/orders/detail
    let resp_ok = client
        .get(format!(
            "http://127.0.0.1:{}/api/v1/orders/detail?id=ord_std_10001",
            port
        ))
        .send()
        .await
        .expect("Mock server should respond to detail query");
    assert_eq!(resp_ok.status().as_u16(), 200);
    let text = resp_ok.text().await.unwrap();
    assert!(text.contains("ord_std_10001"));
    assert!(text.contains("SF1002345678"));

    // 2. 测试 404 分支
    let resp_404 = client
        .get(format!(
            "http://127.0.0.1:{}/api/v1/orders/detail?id=ord_non_exist",
            port
        ))
        .send()
        .await
        .expect("Mock server should respond to 404 query");
    assert_eq!(resp_404.status().as_u16(), 404);
    let text_404 = resp_404.text().await.unwrap();
    assert!(text_404.contains("NotFound"));
}

#[tokio::test]
async fn test_scenario_iam_tenant_mock_server_flow() {
    let spec_path = PathBuf::from("examples/scenario_iam_tenant/iam_spec.md");
    let parsed_spec = MarkdownFileParser::parse_file(&spec_path).unwrap();

    let routes = MockCompiler::compile(&parsed_spec);
    let mut matcher = TrieRouteMatcher::new();
    for r in routes {
        matcher.add_route(&r.method, &r.path, r.variants);
    }
    let matcher = Arc::new(matcher);
    let port = 19189;

    let server = AxumMockServer;
    tokio::spawn(async move {
        let _ = server.start(port, matcher).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let client = reqwest::Client::new();

    // 1. 测试纯契约块 POST /api/v1/tenants/users: 正常创建返回 201
    let resp_create = client
        .post(format!("http://127.0.0.1:{}/api/v1/tenants/users", port))
        .header("X-Tenant-ID", "corp_alpha")
        .header("Content-Type", "application/json")
        .body(r#"{"email": "test@corp.com"}"#)
        .send()
        .await
        .expect("Mock server should create user");
    assert_eq!(resp_create.status().as_u16(), 201);
    let text = resp_create.text().await.unwrap();
    assert!(text.contains("usr_tenant_9901"));

    // 2. 测试缺少租户头返回 400
    let resp_missing_tenant = client
        .post(format!("http://127.0.0.1:{}/api/v1/tenants/users", port))
        .header("Content-Type", "application/json")
        .body(r#"{"email": "test@corp.com"}"#)
        .send()
        .await
        .expect("Mock server should reject missing tenant");
    assert_eq!(resp_missing_tenant.status().as_u16(), 400);
    assert!(
        resp_missing_tenant
            .text()
            .await
            .unwrap()
            .contains("MissingTenantContext")
    );

    // 3. 测试邮箱冲突返回 409
    let resp_conflict = client
        .post(format!(
            "http://127.0.0.1:{}/api/v1/tenants/users?conflict=true",
            port
        ))
        .header("X-Tenant-ID", "corp_alpha")
        .header("Content-Type", "application/json")
        .body(r#"{"email": "alex@corp.com"}"#)
        .send()
        .await
        .expect("Mock server should handle conflict");
    assert_eq!(resp_conflict.status().as_u16(), 409);
    assert!(
        resp_conflict
            .text()
            .await
            .unwrap()
            .contains("UserAlreadyExists")
    );
}
