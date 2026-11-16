use crate::mock::compiler::MockCompiler;
use crate::mock::matcher::TrieRouteMatcher;
use crate::mock::variant::{CompareOp, ConditionSource, EvalContext};
use crate::parser::{MarkdownFileParser, parse_document_by_path};
use crate::variable::{VariableContext, VariableResolver};
use crate::web::mock_manager::discover_workspace_mock_routes;
use crate::web::server::AppState;
use crate::web::types::{
    ApiResponse, ConditionEvaluation, MockSimulateRequest, MockSimulateResponse, StartMockRequest,
    StopMockRequest,
};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use std::collections::HashMap;
use tokio::task::spawn_blocking;

pub async fn simulate_mock_handler(
    State(_state): State<AppState>,
    Json(req): Json<MockSimulateRequest>,
) -> impl IntoResponse {
    let parsed_res = if let Some(ref file_path) = req.file_path {
        if req.raw_block.contains("```") {
            MarkdownFileParser::parse_content(&req.raw_block)
        } else {
            parse_document_by_path(file_path, &req.raw_block)
        }
    } else {
        let markdown_payload = if req.raw_block.contains("```") {
            req.raw_block.clone()
        } else {
            format!("```http\n{}\n```", req.raw_block.trim())
        };
        MarkdownFileParser::parse_content(&markdown_payload)
    };

    let parsed_file = match parsed_res {
        Ok(f) => f,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::err(format!("Failed to parse mock block: {}", e))),
            );
        }
    };

    let routes = match MockCompiler::try_compile(&parsed_file) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::err(e.to_string())),
            );
        }
    };
    if routes.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::err(
                "No mock route definition found in the provided block",
            )),
        );
    }

    let mut matcher = TrieRouteMatcher::new();
    for r in routes {
        matcher.add_route(&r.method, &r.path, r.variants);
    }

    let Some((pattern, variants, captured_params)) =
        matcher.match_route_with_params(&req.method, &req.path)
    else {
        return (
            StatusCode::OK,
            Json(ApiResponse::ok(MockSimulateResponse {
                matched_variant_index: None,
                matched_variant_name: "unmatched".to_string(),
                expected_status: 404,
                evaluations: Vec::new(),
                matched_pattern: None,
                captured_params: HashMap::new(),
                response_headers: HashMap::new(),
                response_body: None,
            })),
        );
    };

    let body_str = req.body.as_deref().unwrap_or("");
    let eval_ctx = EvalContext::new(&req.headers, &req.query, body_str);
    let mut evaluations = Vec::new();
    let mut matched_index = None;
    let mut matched_name = "unmatched".to_string();
    let mut expected_status = 404;
    let mut matched_variant = None;

    for (idx, variant) in variants.iter().enumerate() {
        if let Some(cond) = &variant.condition {
            let detail = cond.evaluate_with_detail(&eval_ctx);
            let source_desc = match cond.source {
                ConditionSource::Header => "header",
                ConditionSource::Query => "query",
                ConditionSource::Body => "body",
            };

            let op_str = match cond.operator {
                CompareOp::Equals => "==",
                CompareOp::Contains => "contains",
                CompareOp::Exists => "exists",
            };

            evaluations.push(ConditionEvaluation {
                condition_str: format!(
                    "[{}] {} {} {}",
                    source_desc, cond.key, op_str, cond.expected_value
                ),
                passed: detail.passed,
                expected: cond.expected_value.clone(),
                actual: detail.actual,
            });

            if detail.passed && matched_index.is_none() {
                matched_index = Some(idx);
                matched_name = format!("variant_{}", idx + 1);
                expected_status = variant.status;
                matched_variant = Some(variant);
            }
        } else if matched_index.is_none() {
            matched_index = Some(idx);
            matched_name = "default".to_string();
            expected_status = variant.status;
            matched_variant = Some(variant);
        }
    }

    let (response_headers, response_body) = if let Some(var) = matched_variant {
        let mut var_ctx = VariableContext::new();
        for (k, v) in &captured_params {
            var_ctx.insert(k, v);
        }
        let mut headers = HashMap::new();
        for (k, v) in &var.headers {
            headers.insert(k.clone(), VariableResolver::resolve(v, &var_ctx));
        }
        let body = VariableResolver::resolve(&var.response_body, &var_ctx);
        (headers, Some(body))
    } else {
        (HashMap::new(), None)
    };

    let response = MockSimulateResponse {
        matched_variant_index: matched_index,
        matched_variant_name: matched_name,
        expected_status,
        evaluations,
        matched_pattern: Some(pattern.to_string()),
        captured_params,
        response_headers,
        response_body,
    };

    (StatusCode::OK, Json(ApiResponse::ok(response)))
}

/// 列出工作区内所有包含 Mock 变体的文档路由（只读模式可用）
pub async fn list_mock_routes_handler(State(state): State<AppState>) -> impl IntoResponse {
    let working_dir = state.working_dir.clone();
    let routes = spawn_blocking(move || discover_workspace_mock_routes(&working_dir))
        .await
        .unwrap_or_default();
    (StatusCode::OK, Json(ApiResponse::ok(routes)))
}

/// 列出当前通过 Web 托管运行中的 Mock 服务实例
pub async fn list_mock_servers_handler(State(state): State<AppState>) -> impl IntoResponse {
    let servers = state.mock_manager.list_servers();
    (StatusCode::OK, Json(ApiResponse::ok(servers)))
}

/// 启动托管 Mock HTTP 服务（只读模式禁止，仅绑定 127.0.0.1，经 PathSandbox 校验）
pub async fn start_mock_server_handler(
    State(state): State<AppState>,
    Json(req): Json<StartMockRequest>,
) -> impl IntoResponse {
    if state.readonly {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::err(
                "Starting mock servers is disabled in read-only mode",
            )),
        );
    }

    match state
        .mock_manager
        .start_server(
            &state.working_dir,
            &req.file_path,
            req.port,
            req.enable_cors.unwrap_or(true),
        )
        .await
    {
        Ok(info) => (StatusCode::CREATED, Json(ApiResponse::ok(info))),
        Err(err) => (err.status, Json(ApiResponse::err(err.message))),
    }
}

/// 停止托管 Mock HTTP 服务（只读模式禁止）
pub async fn stop_mock_server_handler(
    State(state): State<AppState>,
    Json(req): Json<StopMockRequest>,
) -> impl IntoResponse {
    if state.readonly {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::err(
                "Stopping mock servers is disabled in read-only mode",
            )),
        );
    }

    match state.mock_manager.stop_server(req.id.as_deref(), req.port) {
        Ok(info) => (StatusCode::OK, Json(ApiResponse::ok(info))),
        Err(err) => (err.status, Json(ApiResponse::err(err.message))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::server::{WebServerConfig, create_web_router};
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use tower::ServiceExt;

    async fn call_simulate(req: MockSimulateRequest) -> MockSimulateResponse {
        let app = create_web_router(WebServerConfig::default());
        let http_req = Request::builder()
            .method("POST")
            .uri("/api/v1/mock/simulate")
            .header("host", "127.0.0.1")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&req).unwrap()))
            .unwrap();
        let resp = app.oneshot(http_req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let api: ApiResponse<MockSimulateResponse> = serde_json::from_slice(&bytes).unwrap();
        assert!(api.success, "simulate failed: {:?}", api.error);
        api.data.unwrap()
    }

    #[tokio::test]
    async fn test_simulate_captures_params_and_renders_body_with_base_url() {
        let res = call_simulate(MockSimulateRequest {
            method: "GET".to_string(),
            path: "/api/users/42".to_string(),
            raw_block: "GET {{base_url}}/api/users/:id?debug=1\n\n@mock-default\nHTTP/1.1 200 OK\nX-User: {{id}}\n\n{\"id\":\"{{id}}\"}\n".to_string(),
            file_path: Some("api.http".to_string()),
            ..Default::default()
        })
        .await;

        assert_eq!(res.matched_pattern.as_deref(), Some("/api/users/:id"));
        assert_eq!(res.captured_params.get("id").map(String::as_str), Some("42"));
        assert_eq!(res.expected_status, 200);
        assert_eq!(res.response_headers.get("X-User").map(String::as_str), Some("42"));
        assert_eq!(res.response_body.as_deref(), Some("{\"id\":\"42\"}"));
    }

    #[tokio::test]
    async fn test_simulate_unmatched_path_returns_404_unmatched() {
        let res = call_simulate(MockSimulateRequest {
            method: "GET".to_string(),
            path: "/api/not-exist".to_string(),
            raw_block: "GET /api/users/:id\n\n@mock-default\nHTTP/1.1 200 OK\n\nok\n".to_string(),
            ..Default::default()
        })
        .await;

        assert!(res.matched_pattern.is_none());
        assert!(res.matched_variant_index.is_none());
        assert_eq!(res.matched_variant_name, "unmatched");
        assert_eq!(res.expected_status, 404);
        assert!(res.response_body.is_none());
    }

    #[tokio::test]
    async fn test_simulate_when_no_variant_matches_returns_404_unmatched() {
        let res = call_simulate(MockSimulateRequest {
            method: "GET".to_string(),
            path: "/api/orders".to_string(),
            headers: HashMap::new(),
            raw_block: "GET /api/orders\n\n@mock-when header.X-Role == admin\nHTTP/1.1 200 OK\n\nok\n".to_string(),
            ..Default::default()
        })
        .await;

        assert_eq!(res.matched_pattern.as_deref(), Some("/api/orders"));
        assert!(res.matched_variant_index.is_none());
        assert_eq!(res.matched_variant_name, "unmatched");
        assert_eq!(res.expected_status, 404);
    }
}
