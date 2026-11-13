use crate::parser::parse_document_by_path;
use crate::report::types::{ReportAssertionItem, TimingWaterfall};
use crate::runner::TestExecutor;
use crate::utils::path_guard::is_http_doc;
use crate::variable::{ConfigLoader, VariableContext};
use crate::web::server::AppState;
use crate::web::session::{SessionHandle, session_set_cookie_value};
use crate::web::types::{
    ApiResponse, RunBlockRequest, RunBlockResponse, RunSuiteRequest, RunSuiteResponse,
    SuiteItemResult,
};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::header::{COOKIE, HeaderMap, HeaderValue, SET_COOKIE};
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;

/// 从请求头提取 Cookie 值（无 Cookie / 非法编码一律等价新会话，不泄露原因）
fn cookie_header_of(headers: &HeaderMap) -> Option<&str> {
    headers.get(COOKIE).and_then(|v| v.to_str().ok())
}

/// 会话装载（WP-1 共享 helper 1/2）：解析会话 → 回灌捕获变量（显式
/// extra_variables 恒优先，已存在的键不覆盖）→ 导入上游 cookie 桥态。
/// 返回执行租约，供执行完成后 `session_finish` 持久化增量。
fn session_begin(
    state: &AppState,
    cookie_header: Option<&str>,
    var_context: &mut VariableContext,
    executor: &TestExecutor,
) -> SessionHandle {
    let handle = state.sessions.load_or_create(cookie_header);
    if let Some(state_json) = handle.snapshot.cookie_state.clone() {
        let _ = executor.import_cookie_state(state_json);
    }
    for (k, v) in &handle.snapshot.captured_variables {
        if var_context.get(k).is_none() {
            var_context.insert(k.clone(), v.clone());
        }
    }
    handle
}

/// 会话持久化（WP-1 共享 helper 3/3）：cookie 全量导出替换 + 变量增量合并
/// （跳过 `__` 内部键，仅合并新增/变化值）。短临界区，绝不跨 `.await` 持锁。
fn session_finish(
    state: &AppState,
    handle: &SessionHandle,
    executor: &TestExecutor,
    var_context: &VariableContext,
    pre_run_keys: &HashMap<String, String>,
) {
    let exported = executor.export_cookie_state().ok().flatten();
    let mut deltas = Vec::new();
    for (k, v) in var_context.variables() {
        if k.starts_with("__") {
            continue;
        }
        if pre_run_keys.get(k) != Some(v) {
            deltas.push((k.clone(), v.clone()));
        }
    }
    state
        .sessions
        .persist_execution(&handle.id, exported, deltas);
}

/// 统一响应封装（WP-1 helper 2/3 + WP-R1 滑动续期）：到达会话层的每次执行响应
/// （成功与业务失败）都重发 `Set-Cookie`——浏览器 `Max-Age` 随每次执行重新起算，
/// 与服务端滑动 TTL 对齐，活跃会话不得因固定倒计时在 30 分钟硬过期；
/// 执行前失败（只读拒绝、解析错误等，未创建会话）不附。
fn json_session_response<T: serde::Serialize>(
    status: StatusCode,
    body: ApiResponse<T>,
    handle: Option<&SessionHandle>,
) -> Response {
    let mut resp = (status, Json(body)).into_response();
    if let Some(v) = handle
        .map(|h| session_set_cookie_value(&h.id))
        .and_then(|c| HeaderValue::from_str(&c).ok())
    {
        resp.headers_mut().append(SET_COOKIE, v);
    }
    resp
}

/// 错误路径响应（`ApiResponse::err` 不占用 T，固定以 serde_json::Value 实例化
/// 消除调用点类型歧义）
fn json_err_session_response(
    status: StatusCode,
    message: impl Into<String>,
    handle: Option<&SessionHandle>,
) -> Response {
    json_session_response(
        status,
        ApiResponse::<serde_json::Value>::err(message),
        handle,
    )
}

pub async fn run_block_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RunBlockRequest>,
) -> Response {
    // 0. 只读模式网络发包阻断（防 SSRF 内网嗅探探测：--readonly 下执行与保存双双关闭）。
    // 执行前失败：不创建会话
    if state.readonly {
        return json_err_session_response(
            StatusCode::FORBIDDEN,
            "Request execution is disabled in read-only mode",
            None,
        );
    }

    // 1. 依据文档类型统一解析（复用 parser 门面分发）：
    //    纯 .http 文件原生解析，消除硬编码 Markdown 栅栏包裹的迂回逻辑；
    //    Markdown 块保留裸请求自动包裹 ```http 围栏的兼容行为。
    // 执行前失败：不创建会话
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
            return json_err_session_response(
                StatusCode::BAD_REQUEST,
                format!("Failed to parse request block: {}", e),
                None,
            );
        }
    };

    let target_request = match parsed_file.requests.into_iter().next() {
        Some(r) => r,
        None => {
            return json_err_session_response(
                StatusCode::BAD_REQUEST,
                "No executable HTTP request found in the provided block",
                None,
            );
        }
    };

    // 3. 准备变量执行上下文（配置/env 文件告警走服务端日志——走查缺口 #6）
    let mut cli_vars = Vec::new();
    if let Some(extra) = &req.extra_variables {
        for (k, v) in extra {
            cli_vars.push((k.clone(), v.clone()));
        }
    }
    let target_env = req.env.as_deref().or(state.default_env.as_deref());
    let (mut var_context, loader_warnings) =
        ConfigLoader::load_context(target_env, &cli_vars, None);
    for warning in &loader_warnings {
        tracing::warn!("web run: {warning}");
    }
    var_context.insert("__default_scheme", "http");

    // 4. 原位单块调试执行（无污染内存隔离会话；B-1：共享工厂装配本机插件注册表）。
    // R-4（免重填）+ WP-1（按浏览器会话隔离）：cookie 桥导入 + 捕获变量回灌
    // （显式值恒优先；跨客户端零共享）
    let executor = crate::runner::executor_factory::build_interactive_executor();
    let handle = session_begin(
        &state,
        cookie_header_of(&headers),
        &mut var_context,
        &executor,
    );
    let pre_run_keys = var_context.variables().clone();
    let result = executor
        .execute_one(
            target_request,
            1,
            &mut var_context,
            Some(req.file_path.clone()),
        )
        .await;

    // R-4：cookie 导出回桥 + 变量增量合并（新增/变化的捕获值留待下次请求复用）
    session_finish(&state, &handle, &executor, &var_context, &pre_run_keys);

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

    json_session_response(StatusCode::OK, ApiResponse::ok(response), Some(&handle))
}

/// 整篇活文档套件批量执行：一键跑通全部请求与链路断言，返回聚合汇总指标
pub async fn run_suite_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RunSuiteRequest>,
) -> Response {
    // 0. 只读模式网络发包阻断（防 SSRF）。执行前失败：不创建会话
    if state.readonly {
        return json_err_session_response(
            StatusCode::FORBIDDEN,
            "Suite execution is disabled in read-only mode",
            None,
        );
    }

    // 1. 读取整篇活文档内容。执行前失败：不创建会话
    let content = match state.store.read_document(&req.path).await {
        Ok(c) => c,
        Err(e) => {
            return json_err_session_response(
                StatusCode::NOT_FOUND,
                format!("File not found: {}", e),
                None,
            );
        }
    };

    // 2. 统一门面解析整篇活文档（.md / .http 按扩展名分发）。执行前失败：不创建会话
    let parsed_file = match parse_document_by_path(&req.path, &content) {
        Ok(f) => f,
        Err(e) => {
            return json_err_session_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("Failed to parse document: {}", e),
                None,
            );
        }
    };

    // 统一空文档语义：若文档内无任何可执行请求（如空文档或无 HTTP 块的 Markdown），统一返回 422
    if parsed_file.requests.is_empty() {
        return json_err_session_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Document contains no executable HTTP request blocks",
            None,
        );
    }

    // 3. 准备变量上下文（CLI 变量列表为空：环境级联与 .env 同 CLI 语义）
    let target_env = req.env.as_deref().or(state.default_env.as_deref());
    let (mut var_context, loader_warnings) = ConfigLoader::load_context(target_env, &[], None);
    for warning in &loader_warnings {
        tracing::warn!("web run: {warning}");
    }
    var_context.insert("__default_scheme", "http");

    // 4. 内存隔离会话执行整套测试（ephemeral cookies，不污染本机会话；B-1：共享工厂装配插件注册表）。
    // R-4（免重填）+ WP-1（按浏览器会话隔离）：cookie 桥导入 + 捕获变量回灌
    let start_time = std::time::Instant::now();
    let executor = crate::runner::executor_factory::build_interactive_executor();
    let handle = session_begin(
        &state,
        cookie_header_of(&headers),
        &mut var_context,
        &executor,
    );
    let pre_run_keys = var_context.variables().clone();
    let results = match executor.execute_all(parsed_file, &mut var_context).await {
        Ok(r) => r,
        Err(e) => {
            // 执行期业务失败：已创建的会话仍下发 Set-Cookie（状态为空，下次等价续用）
            return json_err_session_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Execution failed: {}", e),
                Some(&handle),
            );
        }
    };

    // R-4：cookie 导出回桥 + 变量增量合并（同 run_block 语义）
    session_finish(&state, &handle, &executor, &var_context, &pre_run_keys);

    // 5. 聚合统计（skipped 语义与 CLI 报告对齐：不计入通过率分母）
    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;
    let mut items = Vec::new();
    for r in &results {
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

    // WP-R2：故障复现包（仅失败时；服务端管辖路径，客户端零路径输入；浏览器
    // 不是可信 secret 通道——secret_values 恒空，仅键名掩码，masking=secrets 仍强制）
    let mut incident_dir: Option<String> = None;
    if req.incident == Some(true) && failed > 0 {
        let target = state.incident_root.join(format!(
            "{}-{}",
            chrono::Utc::now().format("%Y%m%dT%H%M%S"),
            &uuid::Uuid::new_v4().simple().to_string()[..8],
        ));
        let batch = vec![(std::path::PathBuf::from(&req.path), results)];
        if let Err(e) = crate::history::create_incident_bundle(
            &batch,
            std::slice::from_ref(&req.path),
            state.default_env.as_deref(),
            &[],
            crate::history::model::DEFAULT_MAX_SNAPSHOT_BODY_BYTES,
            &target,
        ) {
            // fail-closed 镜像 CLI exit-2 语义：工件失败不静默
            return json_err_session_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!(
                    "Incident bundle creation failed: {}",
                    e.to_user_friendly_string()
                ),
                Some(&handle),
            );
        }
        incident_dir = Some(crate::utils::path_guard::normalize_posix_path(&target));
    }

    let response = RunSuiteResponse {
        path: req.path,
        total: items.len(),
        passed,
        failed,
        skipped,
        duration_ms: start_time.elapsed().as_millis() as u64,
        items,
        incident_dir,
    };

    json_session_response(StatusCode::OK, ApiResponse::ok(response), Some(&handle))
}
