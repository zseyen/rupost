//! R-4（免重填主线）+ WP-1（会话隔离）：Web 轨道跨请求会话延续守护。
//!
//! 契约：首次 /api/v1/run/block 执行登录块（@capture 落入该浏览器会话）并
//! 下发 `Set-Cookie: rupost_session=...` → 后续请求随浏览器自动携带 Cookie，
//! `{{auth_token}}` 由同会话变量回灌免重填成功；显式 extra_variables 恒优先
//! （防假守护对照）。
//!
//! P0 隔离守护（差异化执行计划 A1）：两个浏览器会话不得互读 Cookie 或
//! @capture 变量——服务端按 `Cookie: rupost_session=<opaque>` 划分会话边界，
//! 首次执行下发 `Set-Cookie`，无 Cookie/非法/过期一律等价新会话。

use axum::body::Body;
use axum::http::{Request, StatusCode};
use rupost::web::server::{WebServerConfig, create_web_router};
use serde_json::json;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// 从响应头提取 `rupost_session` 值（仅取 cookie 名后的 value，忽略属性）
fn extract_session_cookie(res: &axum::response::Response) -> Option<String> {
    res.headers().get_all("set-cookie").iter().find_map(|v| {
        let s = v.to_str().ok()?;
        s.split(';')
            .map(str::trim)
            .find_map(|attr| attr.strip_prefix("rupost_session=").map(str::to_string))
    })
}

/// 带会话 Cookie 的执行入口：cookie 为 Some 时随请求携带
/// `Cookie: rupost_session=<v>`；返回（业务数据, 本次响应下发的 rupost_session 值）
async fn run_block_full(
    app: &axum::Router,
    file_path: &str,
    raw_block: &str,
    extra: Option<serde_json::Value>,
    cookie: Option<&str>,
) -> (serde_json::Value, Option<String>) {
    let payload = json!({
        "file_path": file_path,
        "raw_block": raw_block,
        "env": null,
        "extra_variables": extra,
    });
    let mut builder = Request::builder()
        .uri("/api/v1/run/block")
        .method("POST")
        .header("host", "127.0.0.1")
        .header("content-type", "application/json");
    if let Some(c) = cookie {
        builder = builder.header("cookie", format!("rupost_session={c}"));
    }
    let req = builder
        .body(Body::from(serde_json::to_string(&payload).unwrap()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let session = extract_session_cookie(&res);
    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let wrapped: serde_json::Value = serde_json::from_slice(&body).unwrap();
    (wrapped["data"].clone(), session)
}

/// 既有四参形态（不带会话 Cookie）的便捷封装，返回业务数据
async fn run_block(
    app: &axum::Router,
    file_path: &str,
    raw_block: &str,
    extra: Option<serde_json::Value>,
) -> serde_json::Value {
    run_block_full(app, file_path, raw_block, extra, None)
        .await
        .0
}

/// R-4 主链：登录块捕获 → 下游块免重填成功（mock 依赖捕获值 200，否则 401 防假守护）。
#[tokio::test]
async fn test_web_session_variables_reused_across_blocks() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/login"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"token":"tok-web"}"#))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/orders"))
        .and(wiremock::matchers::header(
            "Authorization",
            "Bearer tok-web",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string("orders-ok"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/orders"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
        .mount(&server)
        .await;

    let config = WebServerConfig::default();
    let app = create_web_router(config);

    // 第一次：登录块（@capture 落入该浏览器会话，服务端下发 Set-Cookie）
    let (login, session_cookie) = run_block_full(
        &app,
        "login.http",
        &format!(
            "### login\nPOST {}/login\nContent-Type: application/json\n\n{{}}\n\n@capture auth_token from body.token\n",
            server.uri()
        ),
        None,
        None,
    )
    .await;
    assert!(login["success"].as_bool().unwrap(), "登录块必须成功");
    let session_cookie = session_cookie.expect("首次执行必须下发 rupost_session 会话 Cookie");

    // 第二次：下游块（浏览器自动携带会话 Cookie，不重跑登录）——{{auth_token}} 由同会话回灌
    let (orders, _) = run_block_full(
        &app,
        "orders.http",
        &format!(
            "### list\nGET {}/orders\nAuthorization: Bearer {{{{auth_token}}}}\n\n# @assert status == 200\n",
            server.uri()
        ),
        None,
        Some(&session_cookie),
    )
    .await;
    assert!(
        orders["success"].as_bool().unwrap(),
        "下游块必须凭会话变量免重填成功: {orders}"
    );
}

/// R-4 防假守护对照：未登录直接跑下游 → 会话无 token → 401 失败（不得伪成功）。
#[tokio::test]
async fn test_web_session_absent_token_fails_honestly() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/orders"))
        .and(wiremock::matchers::header(
            "Authorization",
            "Bearer tok-web",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string("orders-ok"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/orders"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
        .mount(&server)
        .await;

    let config = WebServerConfig::default();
    let app = create_web_router(config);
    let orders = run_block(
        &app,
        "orders.http",
        &format!(
            "### list\nGET {}/orders\nAuthorization: Bearer {{{{auth_token}}}}\n",
            server.uri()
        ),
        None,
    )
    .await;
    assert!(
        !orders["success"].as_bool().unwrap(),
        "无会话变量时必须诚实失败: {orders}"
    );
}

/// R-4 优先级契约：显式 extra_variables 恒优先于会话变量（陈旧值不得覆盖）。
#[tokio::test]
async fn test_web_explicit_variables_override_session() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/login"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"token":"tok-session"}"#))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/probe"))
        .and(wiremock::matchers::header(
            "Authorization",
            "Bearer tok-explicit",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/probe"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
        .mount(&server)
        .await;

    let config = WebServerConfig::default();
    let app = create_web_router(config);
    let (_, session_cookie) = run_block_full(
        &app,
        "login.http",
        &format!(
            "### login\nPOST {}/login\nContent-Type: application/json\n\n{{}}\n\n@capture auth_token from body.token\n",
            server.uri()
        ),
        None,
        None,
    )
    .await;
    let session_cookie = session_cookie.expect("登录执行必须下发会话 Cookie");
    // 显式 extra_variables 与会话变量同键：显式值必须胜出（陈旧 tok-session 不得生效）
    let (probe, _) = run_block_full(
        &app,
        "probe.http",
        &format!(
            "### probe\nGET {}/probe\nAuthorization: Bearer {{{{auth_token}}}}\n\n# @assert status == 200\n",
            server.uri()
        ),
        Some(json!({"auth_token": "tok-explicit"})),
        Some(&session_cookie),
    )
    .await;
    assert!(
        probe["success"].as_bool().unwrap(),
        "显式变量必须覆盖会话值: {probe}"
    );
}

/// P0 隔离守护（A1）：两个浏览器会话不得互读 Cookie 或 @capture 变量。
///
/// Client A 登录捕获 `tok-client-a` 并保存服务端下发的会话 Cookie；
/// Client B 不携带 A 的 Cookie 直接跑下游——若变量从 A 泄漏回灌，
/// mock 命中精确 Bearer 头返回 200（泄漏），隔离时必须 401 诚实失败；
/// A 凭自身会话 Cookie 必须成功。
/// 反假守护：恢复全局共享（删除会话 lookup/隔离）时本测试必须稳定变红。
#[tokio::test]
async fn test_web_session_does_not_share_captured_variables_between_clients() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/login"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"token":"tok-client-a"}"#))
        .mount(&server)
        .await;
    // 精确 Bearer 匹配 200 + 兜底 401：token 泄漏（变量互读）才会假成功
    Mock::given(method("GET"))
        .and(path("/orders"))
        .and(wiremock::matchers::header(
            "Authorization",
            "Bearer tok-client-a",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string("orders-a"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/orders"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
        .mount(&server)
        .await;

    let app = create_web_router(WebServerConfig::default());
    let login_block = format!(
        "### login\nPOST {}/login\nContent-Type: application/json\n\n{{}}\n\n@capture auth_token from body.token\n",
        server.uri()
    );
    let orders_block = format!(
        "### list\nGET {}/orders\nAuthorization: Bearer {{{{auth_token}}}}\n\n# @assert status == 200\n",
        server.uri()
    );

    // Client A 登录，保存自己的会话 Cookie
    let (_a_login, cookie_a) = run_block_full(&app, "login.http", &login_block, None, None).await;
    let cookie_a = cookie_a.expect("Client A 首次执行必须下发 rupost_session 会话 Cookie");

    // Client B 不携带 A 的 Cookie：不得读到 A 的捕获变量（下游必须诚实失败）
    let (b_run, cookie_b) = run_block_full(&app, "orders.http", &orders_block, None, None).await;
    assert!(
        !b_run["success"].as_bool().unwrap(),
        "Client B 不得复用 Client A 的捕获变量: {b_run}"
    );
    // B 首次执行（业务失败也算）必须获得自己独立的会话，且与 A 互不相同
    let cookie_b = cookie_b.expect("Client B 首次执行也必须获得独立会话");
    assert_ne!(cookie_a, cookie_b, "两个客户端的会话 ID 必须相互独立");

    // Client A 凭自身 Cookie 重跑同一请求：必须凭自己的捕获变量成功
    let (a_run, _) =
        run_block_full(&app, "orders.http", &orders_block, None, Some(&cookie_a)).await;
    assert!(
        a_run["success"].as_bool().unwrap(),
        "Client A 凭自身会话必须成功: {a_run}"
    );
}
