use crate::parser::MarkdownFileParser;
use crate::report::types::{ReportAssertionItem, TimingWaterfall};
use crate::runner::TestExecutor;
use crate::variable::ConfigLoader;
use crate::web::server::AppState;
use crate::web::types::{ApiResponse, RunBlockRequest, RunBlockResponse};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use std::collections::HashMap;

pub async fn run_block_handler(
    State(state): State<AppState>,
    Json(req): Json<RunBlockRequest>,
) -> impl IntoResponse {
    // 1. 规范化代码块：如果传入裸 HTTP 请求，自动包裹为 ```http ... ```
    let markdown_payload = if req.raw_block.contains("```") {
        req.raw_block.clone()
    } else {
        format!("```http\n{}\n```", req.raw_block.trim())
    };

    // 2. 解析代码块
    let parsed_file = match MarkdownFileParser::parse_content(&markdown_payload) {
        Ok(f) => f,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::err(format!("Failed to parse request block: {}", e))),
            );
        }
    };

    let target_request = match parsed_file.requests.into_iter().next() {
        Some(r) => r,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::err("No executable HTTP request found in the provided block")),
            );
        }
    };

    // 3. 准备变量执行上下文
    let config = ConfigLoader::find_and_load().unwrap_or_default();
    let mut cli_vars = Vec::new();
    if let Some(extra) = &req.extra_variables {
        for (k, v) in extra {
            cli_vars.push((k.clone(), v.clone()));
        }
    }

    let target_env = req.env.as_deref().or(state.default_env.as_deref());
    let mut var_context = ConfigLoader::build_context(&config, target_env, &cli_vars, Some(".env"));
    var_context.insert("__default_scheme", "http");

    // 4. 原位单块调试执行（采用无污染内存隔离会话）
    let executor = TestExecutor::with_ephemeral_cookies();
    let result = executor
        .execute_one(
            target_request,
            1,
            &mut var_context,
            Some(req.file_path.clone()),
        )
        .await;

    // 5. 提取响应信息与网络瀑布流
    let timing_waterfall = TimingWaterfall::from_test_result(&result);
    let assertions = result
        .assertions
        .iter()
        .map(ReportAssertionItem::from)
        .collect();

    let (response_headers, response_body) = if let Some(resp) = &result.response {
        let mut headers = HashMap::new();
        for (k, v) in &resp.headers {
            headers.insert(k.as_str().to_string(), v.to_str().unwrap_or("").to_string());
        }
        let body = Some(resp.body.clone());
        (headers, body)
    } else {
        (HashMap::new(), None)
    };

    let response = RunBlockResponse {
        success: result.success,
        status: result.status,
        duration_ms: result.duration.as_millis() as u64,
        assertions,
        timing_waterfall,
        response_headers,
        response_body,
        error: result.error,
        warnings: Vec::new(),
    };

    (StatusCode::OK, Json(ApiResponse::ok(response)))
}
