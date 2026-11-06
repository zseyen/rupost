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

#[tokio::test]
async fn test_scenario_order_fulfillment_dual_role_mock_dispatch() {
    let spec_path = PathBuf::from("examples/scenario_order_fulfillment/order_spec.md");
    let parsed_spec = MarkdownFileParser::parse_file(&spec_path).unwrap();

    // 编译 Mock 路由（由于 Stage 1 修复，双角色块 POST /api/v1/orders 也会被编译进 Mock 树）
    let routes = MockCompiler::compile(&parsed_spec);
    let mut matcher = TrieRouteMatcher::new();
    for r in routes {
        matcher.add_route(&r.method, &r.path, r.variants);
    }
    let matcher = Arc::new(matcher);
    let port = 19190;

    let server = AxumMockServer;
    tokio::spawn(async move {
        let _ = server.start(port, matcher).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let client = reqwest::Client::new();
    let base_url = format!("http://127.0.0.1:{}", port);

    // 1. 触发变体分支 A: 携带 Idempotency-Key == repeat_tx_001 -> 期望 200 OK，返回重复订单 ID
    let resp_idempotent = client
        .post(format!("{}/api/v1/orders", base_url))
        .header("Authorization", "Bearer dev_token")
        .header("Idempotency-Key", "repeat_tx_001")
        .header("Content-Type", "application/json")
        .body(r#"{"item_count": 2, "amount": 29900}"#)
        .send()
        .await
        .expect("Mock server should respond to idempotent order POST");
    assert_eq!(resp_idempotent.status().as_u16(), 200);
    let json_idempotent: serde_json::Value = resp_idempotent.json().await.unwrap();
    assert_eq!(json_idempotent["id"], "ord_existing_8888");
    assert_eq!(json_idempotent["duplicated"], true);

    // 2. 触发变体分支 B: 携带 Query channel == vip -> 期望 201 Created，返回 VIP 折扣单
    let resp_vip = client
        .post(format!("{}/api/v1/orders?channel=vip", base_url))
        .header("Authorization", "Bearer dev_token")
        .header("Content-Type", "application/json")
        .body(r#"{"item_count": 2, "amount": 26910}"#)
        .send()
        .await
        .expect("Mock server should respond to VIP order POST");
    assert_eq!(resp_vip.status().as_u16(), 201);
    let json_vip: serde_json::Value = resp_vip.json().await.unwrap();
    assert_eq!(json_vip["id"], "ord_vip_9999");
    assert_eq!(json_vip["channel"], "vip");
    assert_eq!(json_vip["discount_applied"], true);

    // 3. 触发变体分支 C: 不带 Authorization Header -> 期望 401 Unauthorized
    let resp_unauth = client
        .post(format!("{}/api/v1/orders", base_url))
        .header("Content-Type", "application/json")
        .body(r#"{"item_count": 1}"#)
        .send()
        .await
        .expect("Mock server should respond to unauthorized POST");
    assert_eq!(resp_unauth.status().as_u16(), 401);
    let json_unauth: serde_json::Value = resp_unauth.json().await.unwrap();
    assert_eq!(json_unauth["error"], "Unauthorized");

    // 4. 触发默认变体分支 D (@mock-default): 正常下单 -> 期望 201 Created，标准单号
    let resp_default = client
        .post(format!("{}/api/v1/orders", base_url))
        .header("Authorization", "Bearer dev_token")
        .header("Content-Type", "application/json")
        .body(r#"{"item_count": 2, "amount": 29900}"#)
        .send()
        .await
        .expect("Mock server should respond to standard order POST");
    assert_eq!(resp_default.status().as_u16(), 201);
    let json_default: serde_json::Value = resp_default.json().await.unwrap();
    assert_eq!(json_default["id"], "ord_std_10001");
    assert_eq!(json_default["duplicated"], false);
}

#[tokio::test]
async fn test_scenario_order_spec_as_executable_test() {
    let spec_path = PathBuf::from("examples/scenario_order_fulfillment/order_spec.md");
    let parsed_spec = MarkdownFileParser::parse_file(&spec_path).unwrap();

    // 先启动订单 Mock 服务提供后端依赖
    let routes = MockCompiler::compile(&parsed_spec);
    let mut matcher = TrieRouteMatcher::new();
    for r in routes {
        matcher.add_route(&r.method, &r.path, r.variants);
    }
    let matcher = Arc::new(matcher);
    let port = 19191;

    let server = AxumMockServer;
    tokio::spawn(async move {
        let _ = server.start(port, matcher).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // 模拟 rupost test order_spec.md：使用 TestExecutor 运行文档
    let executor = rupost::runner::TestExecutor::new();
    let mut context = rupost::variable::VariableContext::new();
    context.insert("base_url", format!("http://127.0.0.1:{}", port));
    context.insert("auth_token", "dev_token_for_self_test");

    let results = executor
        .execute_all(parsed_spec, &mut context)
        .await
        .expect("Execution should succeed");

    // 校验结果：
    // 请求 1: POST /api/v1/orders 带有 @test -> 必须执行并通过全部断言
    assert_eq!(results.len(), 2);
    let res_create = &results[0];
    assert!(
        res_create.success,
        "双角色下单用例自检必须通过，error: {:?}",
        res_create.error
    );
    assert_eq!(res_create.assertions.len(), 2);
    assert!(res_create.assertions.iter().all(|a| a.passed));

    // 请求 2: GET /api/v1/orders/detail 无 @test -> 必须被自动跳过 (skipped)，不可盲目发包
    let res_detail = &results[1];
    assert!(
        res_detail.skipped,
        "纯契约块无 @test，在测试模式下必须被跳过 (skipped)"
    );
}

#[tokio::test]
async fn test_scenario_order_suite_runs_against_mock_server() {
    let spec_path = PathBuf::from("examples/scenario_order_fulfillment/order_spec.md");
    let parsed_spec = MarkdownFileParser::parse_file(&spec_path).unwrap();

    // 启动订单 Mock 服务
    let routes = MockCompiler::compile(&parsed_spec);
    let mut matcher = TrieRouteMatcher::new();
    for r in routes {
        matcher.add_route(&r.method, &r.path, r.variants);
    }
    let matcher = Arc::new(matcher);
    let port = 19192;

    let server = AxumMockServer;
    tokio::spawn(async move {
        let _ = server.start(port, matcher).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // 解析测试套件 order_test_suite.http
    let suite_path = PathBuf::from("examples/scenario_order_fulfillment/order_test_suite.http");
    let parsed_suite = HttpFileParser::parse_file(&suite_path).unwrap();

    let executor = rupost::runner::TestExecutor::new();
    let mut context = rupost::variable::VariableContext::new();
    context.insert("base_url", format!("http://127.0.0.1:{}", port));
    context.insert("auth_token", "dev_order_admin_token_2026");

    let results = executor
        .execute_all(parsed_suite, &mut context)
        .await
        .expect("Test suite execution should complete");

    // order_test_suite.http 共 7 条用例：
    // 用例 1-6 针对本地 Mock 服务（下单、幂等重放、VIP折扣、401未鉴权、详情查询、404查询）
    // 用例 7 是针对 httpbin.org 的第三方调用（打有 # @skip 且为外网，离线自测自动跳过）
    assert_eq!(results.len(), 7);

    // 验证前 6 条用例在 Mock 服务下全部执行并通过
    for (idx, res) in results.iter().take(6).enumerate() {
        assert!(
            res.success,
            "用例 #{} ({}) 执行失败: error: {:?}, status: {:?}",
            idx + 1,
            res.url,
            res.error,
            res.status
        );
        assert!(!res.assertions.is_empty(), "用例 #{} 必须包含断言", idx + 1);
        assert!(
            res.assertions.iter().all(|a| a.passed),
            "用例 #{} 的全部断言必须通过",
            idx + 1
        );
    }

    // 验证第 7 条（打上了 # @skip 的外网逃逸用例）被正确标记为跳过
    let res_escape = &results[6];
    assert!(
        res_escape.skipped,
        "外网逃逸且标有 @skip 的用例必须保持跳过"
    );
}
