//! R-4（免重填主线）：Web 轨道跨请求会话延续守护。
//!
//! 契约：服务生命周期内，第一次 /api/v1/run/block 执行登录块（@capture 落入
//! AppState.run_variables）→ 第二次执行下游块（不重跑登录）时 `{{auth_token}}`
//! 由会话变量回灌免重填成功；显式 extra_variables 恒优先（防假守护对照）。

use axum::body::Body;
use axum::http::{Request, StatusCode};
use rupost::web::server::{WebServerConfig, create_web_router};
use serde_json::json;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn run_block(
    app: &axum::Router,
    file_path: &str,
    raw_block: &str,
    extra: Option<serde_json::Value>,
) -> serde_json::Value {
    let payload = json!({
        "file_path": file_path,
        "raw_block": raw_block,
        "env": null,
        "extra_variables": extra,
    });
    let req = Request::builder()
        .uri("/api/v1/run/block")
        .method("POST")
        .header("host", "127.0.0.1")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&payload).unwrap()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let wrapped: serde_json::Value = serde_json::from_slice(&body).unwrap();
    wrapped["data"].clone()
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

    // 第一次：登录块（@capture 落入会话变量）
    let login = run_block(
        &app,
        "login.http",
        &format!(
            "### login\nPOST {}/login\nContent-Type: application/json\n\n{{}}\n\n@capture auth_token from body.token\n",
            server.uri()
        ),
        None,
    )
    .await;
    assert!(login["success"].as_bool().unwrap(), "登录块必须成功");

    // 第二次：下游块（不重跑登录）——{{auth_token}} 由会话回灌
    let orders = run_block(
        &app,
        "orders.http",
        &format!(
            "### list\nGET {}/orders\nAuthorization: Bearer {{{{auth_token}}}}\n\n# @assert status == 200\n",
            server.uri()
        ),
        None,
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
    let _login = run_block(
        &app,
        "login.http",
        &format!(
            "### login\nPOST {}/login\nContent-Type: application/json\n\n{{}}\n\n@capture auth_token from body.token\n",
            server.uri()
        ),
        None,
    )
    .await;
    let probe = run_block(
        &app,
        "probe.http",
        &format!(
            "### probe\nGET {}/probe\nAuthorization: Bearer {{{{auth_token}}}}\n\n# @assert status == 200\n",
            server.uri()
        ),
        Some(json!({"auth_token": "tok-explicit"})),
    )
    .await;
    assert!(
        probe["success"].as_bool().unwrap(),
        "显式变量必须覆盖会话值: {probe}"
    );
}
