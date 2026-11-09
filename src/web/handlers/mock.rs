use crate::mock::compiler::MockCompiler;
use crate::mock::variant::{CompareOp, ConditionSource};
use crate::parser::MarkdownFileParser;
use crate::web::server::AppState;
use crate::web::types::{
    ApiResponse, ConditionEvaluation, MockSimulateRequest, MockSimulateResponse,
};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;

pub async fn simulate_mock_handler(
    State(_state): State<AppState>,
    Json(req): Json<MockSimulateRequest>,
) -> impl IntoResponse {
    let markdown_payload = if req.raw_block.contains("```") {
        req.raw_block.clone()
    } else {
        format!("```http\n{}\n```", req.raw_block.trim())
    };

    let parsed_file = match MarkdownFileParser::parse_content(&markdown_payload) {
        Ok(f) => f,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::err(format!(
                    "Failed to parse mock block: {}",
                    e
                ))),
            );
        }
    };

    let routes = MockCompiler::compile(&parsed_file);
    if routes.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::err(
                "No mock route definition found in the provided block",
            )),
        );
    }

    // 优先匹配方法与路径，兜底取第一个
    let route = routes
        .iter()
        .find(|r| {
            r.method.eq_ignore_ascii_case(&req.method)
                && (r.path == req.path
                    || r.path.trim_start_matches('/') == req.path.trim_start_matches('/'))
        })
        .unwrap_or(&routes[0]);

    let body_str = req.body.as_deref().unwrap_or("");
    let mut evaluations = Vec::new();
    let mut matched_index = None;
    let mut matched_name = "default".to_string();
    let mut expected_status = 200;

    for (idx, variant) in route.variants.iter().enumerate() {
        if let Some(cond) = &variant.condition {
            let passed = cond.evaluate(&req.headers, &req.query, body_str);

            let (source_desc, actual_val) = match cond.source {
                ConditionSource::Header => {
                    let val = req
                        .headers
                        .get(&cond.key)
                        .or_else(|| {
                            req.headers
                                .iter()
                                .find(|(k, _)| k.eq_ignore_ascii_case(&cond.key))
                                .map(|(_, v)| v)
                        })
                        .cloned();
                    ("header", val)
                }
                ConditionSource::Query => {
                    let val = req.query.get(&cond.key).cloned();
                    ("query", val)
                }
                ConditionSource::Body => {
                    let val = if cond.key.starts_with('$') {
                        serde_json::from_str::<serde_json::Value>(body_str)
                            .ok()
                            .and_then(|json| {
                                crate::utils::jsonpath::JsonPathResolver::resolve(&json, &cond.key)
                                    .map(|v| match v {
                                        serde_json::Value::String(s) => s.clone(),
                                        other => other.to_string(),
                                    })
                            })
                    } else {
                        Some(body_str.to_string())
                    };
                    ("body", val)
                }
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
                passed,
                expected: cond.expected_value.clone(),
                actual: actual_val,
            });

            if passed && matched_index.is_none() {
                matched_index = Some(idx);
                matched_name = format!("variant_{}", idx + 1);
                expected_status = variant.status;
            }
        } else if matched_index.is_none() {
            // 无条件变体作为兜底匹配
            matched_index = Some(idx);
            matched_name = "default".to_string();
            expected_status = variant.status;
        }
    }

    let response = MockSimulateResponse {
        matched_variant_index: matched_index,
        matched_variant_name: matched_name,
        expected_status,
        evaluations,
    };

    (StatusCode::OK, Json(ApiResponse::ok(response)))
}
