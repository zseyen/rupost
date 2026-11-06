// 进程级静音历史落盘
mod common;

use futures_util::{SinkExt, StreamExt};
use rupost::mock::compiler::MockCompiler;
use rupost::mock::matcher::TrieRouteMatcher;
use rupost::mock::server::{AxumMockServer, MockServer};
use rupost::parser::{HttpFileParser, MarkdownFileParser};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::protocol::Message;

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

// 辅助工具：启动场景 C 专属的本地 Mock WebSocket 服务
async fn start_scenario_c_ws_server() -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let ws_url = format!("ws://127.0.0.1:{}", port);

    let handle = tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await
            && let Ok(mut ws_stream) = tokio_tungstenite::accept_async(stream).await
        {
            let mut heartbeat_timer = tokio::time::interval(Duration::from_millis(80));

            loop {
                tokio::select! {
                    // 模拟高频心跳广播帧，验证引擎软过滤
                    _ = heartbeat_timer.tick() => {
                        let heartbeat_msg = Message::Text(r#"{"type":"heartbeat"}"#.to_string());
                        if ws_stream.send(heartbeat_msg).await.is_err() {
                            break;
                        }
                    }
                    maybe_msg = ws_stream.next() => {
                        match maybe_msg {
                            Some(Ok(Message::Text(txt))) => {
                                if txt.contains(r#""action":"join""#) {
                                    let resp = Message::Text(r#"{"event":"user_joined","user":"designer_a"}"#.to_string());
                                    if ws_stream.send(resp).await.is_err() {
                                        break;
                                    }
                                } else if txt.contains(r#""action":"draw""#) {
                                    let resp = Message::Text(r#"{"event":"shape_added","shape":"rectangle"}"#.to_string());
                                    if ws_stream.send(resp).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            Some(Ok(Message::Close(_))) | None => break,
                            _ => {}
                        }
                    }
                }
            }
        }
    });

    (ws_url, handle)
}

#[test]
fn test_scenario_c_spec_structure_and_mock_compilation() {
    let spec_path = PathBuf::from("examples/scenarios/scenario_c_ai_collaboration/ai_spec.md");
    let parsed_spec =
        MarkdownFileParser::parse_file(&spec_path).expect("ai_spec.md should parse cleanly");

    // 验证包含 2 个核心协议块：SSE 和 WebSocket
    assert_eq!(
        parsed_spec.requests.len(),
        2,
        "场景 C 必须包含 SSE 聊天和 WS 协同两个协议块"
    );

    // 验证请求 1：AI 聊天流式补全 (SSE)
    let sse_req = &parsed_spec.requests[0];
    assert_eq!(sse_req.method.as_deref(), Some("POST"));
    assert_eq!(sse_req.url, "/api/v1/chat/completions");
    assert!(sse_req.metadata.sse, "必须标注 @sse");
    assert_eq!(
        sse_req.metadata.sse_timeout,
        Some(Duration::from_secs(5)),
        "必须标注 @sse_timeout 5s"
    );
    assert_eq!(
        sse_req.metadata.stream_to,
        Some("test_scratch/scenario_c_chat.txt".to_string()),
        "必须标注 @stream_to 路径"
    );

    // 验证请求 2：白板协同 (WebSocket)
    let ws_req = &parsed_spec.requests[1];
    assert_eq!(ws_req.method.as_deref(), Some("GET"));
    assert_eq!(ws_req.url, "/api/v1/collaboration/canvas_101");
    assert!(ws_req.metadata.websocket, "必须标注 @websocket");
    assert_eq!(
        ws_req.metadata.timeout,
        Some(Duration::from_secs(5)),
        "必须标注 @timeout 5s"
    );
    assert_eq!(ws_req.metadata.decoder.as_deref(), Some("json"));
    let ws_body = ws_req.body.as_deref().unwrap_or("");
    assert!(
        ws_body.contains("SEND") && ws_body.contains("EXPECT") && ws_body.contains("CLOSE"),
        "WebSocket 块必须包含完整的 SEND/EXPECT/CLOSE 剧本"
    );

    // 编译 Mock 路由
    let routes = MockCompiler::compile(&parsed_spec);
    // 注意：目前 Mock 编译器仅编译具有 mock_variants 的请求（即 SSE 聊天块）
    assert_eq!(routes.len(), 1, "SSE 聊天块必须成功编译为 Mock 路由");
    let route = &routes[0];
    assert_eq!(route.path, "/api/v1/chat/completions");
    assert_eq!(route.variants.len(), 2, "必须包含默认 200 和 429 两个变体");
}

#[tokio::test]
async fn test_scenario_c_suite_full_execution_against_mock_servers() {
    let spec_path = PathBuf::from("examples/scenarios/scenario_c_ai_collaboration/ai_spec.md");
    let parsed_spec = MarkdownFileParser::parse_file(&spec_path).unwrap();

    // 1. 启动本地 SSE Mock 服务（基于 ai_spec.md 编译出的路由）
    let routes = MockCompiler::compile(&parsed_spec);
    let mut matcher = TrieRouteMatcher::new();
    for r in routes {
        matcher.add_route(&r.method, &r.path, r.variants);
    }
    let matcher = Arc::new(matcher);
    let http_port = 19194;

    let server = AxumMockServer;
    tokio::spawn(async move {
        let _ = server.start(http_port, matcher).await;
    });
    tokio::time::sleep(Duration::from_millis(80)).await;

    // 2. 启动本地 WS 协同 Mock 服务
    let (ws_url, _ws_handle) = start_scenario_c_ws_server().await;

    // 确保测试落盘目录存在，并清理残留文件
    let scratch_file = PathBuf::from("test_scratch/scenario_c_chat.txt");
    if let Some(parent) = scratch_file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::remove_file(&scratch_file);

    // 3. 解析并运行 ai_test_suite.http
    let suite_path =
        PathBuf::from("examples/scenarios/scenario_c_ai_collaboration/ai_test_suite.http");
    let parsed_suite = HttpFileParser::parse_file(&suite_path).unwrap();

    let executor = rupost::runner::TestExecutor::new();
    let mut context = rupost::variable::VariableContext::new();
    context.insert("host", format!("http://127.0.0.1:{}", http_port));
    context.insert("ws_host", ws_url);

    let results = executor
        .execute_all(parsed_suite, &mut context)
        .await
        .expect("Scenario C test suite should execute cleanly");

    assert_eq!(results.len(), 3, "套件应包含 3 个测试用例");

    // 验证用例 1: SSE 流式打字机
    let sse_result = &results[0];
    assert!(
        sse_result.success,
        "SSE 用例必须执行成功，error: {:?}",
        sse_result.error
    );
    assert_eq!(sse_result.status, Some(200));
    assert!(!sse_result.assertions.is_empty(), "SSE 用例必须包含断言");
    assert!(
        sse_result.assertions.iter().all(|a| a.passed),
        "SSE 用例的所有断言必须全部 PASS: {:?}",
        sse_result.assertions
    );
    // 验证 @stream_to 文件实时落盘
    assert!(
        scratch_file.exists(),
        "已标注 @stream_to 的落盘文件必须被创建"
    );
    let dumped_content = std::fs::read_to_string(&scratch_file).unwrap_or_default();
    assert!(
        dumped_content.contains("Hello from Rupost!"),
        "落盘文件必须包含增量累积的内容，实际内容: {}",
        dumped_content
    );

    // 验证用例 2: 429 限流降级
    let rate_limit_result = &results[1];
    assert!(
        rate_limit_result.success,
        "429 降级用例必须执行成功，error: {:?}",
        rate_limit_result.error
    );
    assert_eq!(rate_limit_result.status, Some(429));
    assert!(
        rate_limit_result.assertions.iter().all(|a| a.passed),
        "429 降级用例的所有断言必须全部 PASS: {:?}",
        rate_limit_result.assertions
    );

    // 验证用例 3: WebSocket 全双工协同
    let ws_result = &results[2];
    assert!(
        ws_result.success,
        "WebSocket 协同用例必须执行成功，error: {:?}",
        ws_result.error
    );
    assert!(
        ws_result.assertions.iter().all(|a| a.passed),
        "WebSocket 用例的所有动作匹配必须通过: {:?}",
        ws_result.assertions
    );

    // 测试完毕后清理临时文件
    let _ = std::fs::remove_file(&scratch_file);
}
