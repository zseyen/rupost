use crate::parser::parse_document_by_path;
use crate::report::types::{ReportAssertionItem, TimingWaterfall};
use crate::runner::TestExecutor;
use crate::utils::path_guard::is_http_doc;
use crate::variable::ConfigLoader;
use crate::web::server::AppState;
use crate::web::types::{
    ApiResponse, RunBlockRequest, RunBlockResponse, RunSuiteRequest, RunSuiteResponse,
    SuiteItemResult,
};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use std::collections::HashMap;

pub async fn run_block_handler(
    State(state): State<AppState>,
    Json(req): Json<RunBlockRequest>,
) -> impl IntoResponse {
    // 0. 只读模式网络发包阻断（防 SSRF 内网嗅探探测：--readonly 下执行与保存双双关闭）
    if state.readonly {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::err(
                "Request execution is disabled in read-only mode",
            )),
        );
    }

    // 1. 依据文档类型统一解析（复用 parser 门面分发）：
    //    纯 .http 文件原生解析，消除硬编码 Markdown 栅栏包裹的迂回逻辑；
    //    Markdown 块保留裸请求自动包裹 ```http 围栏的兼容行为
    let parse_result = if is_http_doc(&req.file_path) {
        parse_document_by_path(&req.file_path, &req.raw_block)
    } else {
        let markdown_payload = if req.raw_block.contains("```") {
            req.raw_block.clone()
        } else {
            format!("```http\n{}\n```", req.raw_block.trim())
        };
        parse_document_by_path(&req.file_path, &markdown_payload)
    };

    // 2. 解析代码块
    let parsed_file = match parse_result {
        Ok(f) => f,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::err(format!(
                    "Failed to parse request block: {}",
                    e
                ))),
            );
        }
    };

    let target_request = match parsed_file.requests.into_iter().next() {
        Some(r) => r,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::err(
                    "No executable HTTP request found in the provided block",
                )),
            );
        }
    };

    // 3. 准备变量执行上下文（配置/env 文件告警走服务端日志——走查缺口 #6）
    let (config, mut loader_warnings) = ConfigLoader::find_and_load();
    let config = config.unwrap_or_default();
    let mut cli_vars = Vec::new();
    if let Some(extra) = &req.extra_variables {
        for (k, v) in extra {
            cli_vars.push((k.clone(), v.clone()));
        }
    }

    let target_env = req.env.as_deref().or(state.default_env.as_deref());
    let (mut var_context, env_warnings) =
        ConfigLoader::build_context(&config, target_env, &cli_vars, Some((".env", false)));
    loader_warnings.extend(env_warnings);
    for warning in &loader_warnings {
        tracing::warn!("web run: {warning}");
    }
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

/// 整篇活文档套件批量执行：一键跑通全部请求与链路断言，返回聚合汇总指标
pub async fn run_suite_handler(
    State(state): State<AppState>,
    Json(req): Json<RunSuiteRequest>,
) -> impl IntoResponse {
    // 0. 只读模式网络发包阻断（防 SSRF）
    if state.readonly {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::err(
                "Suite execution is disabled in read-only mode",
            )),
        );
    }

    // 1. 读取整篇活文档内容
    let content = match state.store.read_document(&req.path).await {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiResponse::err(format!("File not found: {}", e))),
            );
        }
    };

    // 2. 统一门面解析整篇活文档（.md / .http 按扩展名分发）
    let parsed_file = match parse_document_by_path(&req.path, &content) {
        Ok(f) => f,
        Err(e) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ApiResponse::err(format!("Failed to parse document: {}", e))),
            );
        }
    };

    // 统一空文档语义：若文档内无任何可执行请求（如空文档或无 HTTP 块的 Markdown），统一返回 422
    if parsed_file.requests.is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ApiResponse::err(
                "Document contains no executable HTTP request blocks",
            )),
        );
    }

    // 3. 准备变量上下文（CLI 变量列表为空：环境级联与 .env 同 CLI 语义）
    let (config, mut loader_warnings) = ConfigLoader::find_and_load();
    let config = config.unwrap_or_default();
    let target_env = req.env.as_deref().or(state.default_env.as_deref());
    let (mut var_context, env_warnings) =
        ConfigLoader::build_context(&config, target_env, &[], Some((".env", false)));
    loader_warnings.extend(env_warnings);
    for warning in &loader_warnings {
        tracing::warn!("web run: {warning}");
    }
    var_context.insert("__default_scheme", "http");

    // 4. 内存隔离会话执行整套测试（ephemeral cookies，不污染本机会话）
    let start_time = std::time::Instant::now();
    let executor = TestExecutor::with_ephemeral_cookies();
    let results = match executor.execute_all(parsed_file, &mut var_context).await {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::err(format!("Execution failed: {}", e))),
            );
        }
    };

    // 5. 聚合统计（skipped 语义与 CLI 报告对齐：不计入通过率分母）
    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;
    let mut items = Vec::new();
    for r in results {
        if r.skipped {
            skipped += 1;
        } else if r.success {
            passed += 1;
        } else {
            failed += 1;
        }
        items.push(SuiteItemResult {
            name: r.name.clone(),
            method: r.method.clone(),
            url: r.url.clone(),
            status: r.status,
            duration_ms: r.duration.as_millis() as u64,
            success: r.success,
            skipped: r.skipped,
            error: r.error.clone(),
        });
    }

    let response = RunSuiteResponse {
        path: req.path,
        total: items.len(),
        passed,
        failed,
        skipped,
        duration_ms: start_time.elapsed().as_millis() as u64,
        items,
    };

    (StatusCode::OK, Json(ApiResponse::ok(response)))
}
