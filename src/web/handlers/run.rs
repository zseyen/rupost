use crate::diagnostics::{BufferedWarningSink, WarningSink};
use crate::history::create_incident_bundle;
use crate::history::model::DEFAULT_MAX_SNAPSHOT_BODY_BYTES;
use crate::parser::parse_document_by_path;
use crate::plugin::DiscoverOptions;
use crate::report::types::{ReportAssertionItem, TimingWaterfall};
use crate::runner::TestExecutor;
use crate::runner::executor_factory::build_interactive_executor_with_options_and_arc_sink;
use crate::runner::interactive_session::{
    backfill_session_vars, export_session_cookies, harvest_session_deltas, import_session_cookies,
};
use crate::security::fs::env_flag_enabled;
use crate::utils::path_guard::{is_http_doc, normalize_posix_path};
use crate::variable::{ConfigLoader, PluginTomlConfig, VariableContext, WorkspaceConfig};
use crate::web::server::AppState;
use crate::web::session::{SessionHandle, cookie_header_of, session_set_cookie_value};
use crate::web::types::{
    ApiResponse, PluginOverrides, RunBlockRequest, RunBlockResponse, RunSuiteRequest,
    RunSuiteResponse, SuiteItemResult,
};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::header::{HeaderMap, HeaderValue, SET_COOKIE};
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::task::spawn_blocking;
use uuid::Uuid;

/// WP-R7：Web 复现包保留上限——创建前裁剪最旧（安全分析残余收口；CLI 通道
/// 是用户显式路径，不做保留策略）。并发双跑可轻微超上限：无锁容差，
/// uuid 后缀防命名冲突，超限有界。
const MAX_INCIDENT_BUNDLES: usize = 32;

/// 服务端命名判别：`YYYYMMDDTHHMMSS-8hex`（len==24）。只删匹配命名的目录，
/// 绝不触碰非服务端命名格式的条目（用户手工内容/symlink/文件一律跳过）。
fn is_server_named_incident(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() == 24
        && b[8] == b'T'
        && b[..8].iter().all(u8::is_ascii_digit)
        && b[9..15].iter().all(u8::is_ascii_digit)
        && b[15] == b'-'
        && b[16..24].iter().all(u8::is_ascii_hexdigit)
}

/// 匹配目录数 >= max 时按名称升序（即时间序）删除最旧
fn prune_incident_dirs(root: &Path, max: usize) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let mut named: Vec<OsString> = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.metadata().map(|m| m.is_dir()).unwrap_or(false)
                && is_server_named_incident(&e.file_name().to_string_lossy())
        })
        .map(|e| e.file_name())
        .collect();
    named.sort();
    while named.len() >= max {
        let Some(oldest) = named.first().cloned() else {
            break;
        };
        let _ = std::fs::remove_dir_all(root.join(&oldest));
        named.remove(0);
    }
}

/// 会话装载（WP-1 共享 helper 1/2）：解析会话 → 回灌捕获变量（显式
/// extra_variables 恒优先，已存在的键不覆盖）→ 导入上游 cookie 桥态。
/// 返回执行租约，供执行完成后 `session_finish` 持久化增量。
/// B3（WP-2026-11-14-01）：请求覆盖 > 工作区 toml [plugins] > 默认 的合并（单点 SSOT，
/// run_block/run_suite 共用，直接基于 `WorkspaceConfig.plugins` 叠加，避免二次读盘与 `current_dir` 漂移）。
fn apply_plugin_overrides(
    mut cfg: PluginTomlConfig,
    overrides: Option<&PluginOverrides>,
) -> PluginTomlConfig {
    if let Some(ov) = overrides {
        if let Some(v) = ov.allow_missing_plugins {
            cfg.allow_missing_plugins = v;
        }
        if let Some(v) = ov.strict_plugins {
            cfg.strict_plugins = v;
        }
        if let Some(v) = ov.plugin_timeout_secs {
            cfg.plugin_timeout_secs = Some(v);
        }
        if !ov.plugin_env.is_empty() {
            cfg.plugin_env = ov.plugin_env.clone();
        }
        if let Some(v) = ov.strict_vars {
            cfg.strict_vars = v;
        }
        if let Some(v) = ov.allow_project_plugins {
            cfg.allow_project_plugins = v;
        }
    }
    cfg
}

/// B3 strict 门禁：严格模式下降级形态的成功 → 判失败（与 CLI --strict-plugins 语义一致）
fn apply_strict_plugins_gate(success: &mut bool, degraded: bool, strict_plugins: bool) {
    if strict_plugins && degraded && *success {
        *success = false;
    }
}

fn session_begin(
    state: &AppState,
    cookie_header: Option<&str>,
    var_context: &mut VariableContext,
    executor: &TestExecutor,
) -> SessionHandle {
    let handle = state.sessions.load_or_create(cookie_header);
    import_session_cookies(executor, handle.snapshot.cookie_state.clone());
    backfill_session_vars(var_context, &handle.snapshot.captured_variables);
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
    let exported = export_session_cookies(executor);
    let deltas = harvest_session_deltas(var_context, pre_run_keys);
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

/// 装配 Web 交互执行器（请求覆盖 > toml [plugins] > 默认，挂载 BufferedWarningSink）。
fn build_web_executor(
    working_dir: &Path,
    overrides: Option<&PluginOverrides>,
) -> (TestExecutor, PluginTomlConfig, Arc<BufferedWarningSink>) {
    let sink = Arc::new(BufferedWarningSink::new());
    let ws_cfg = WorkspaceConfig::load(working_dir, sink.as_ref());
    let plugin_cfg = apply_plugin_overrides(ws_cfg.plugins, overrides);
    let executor = build_interactive_executor_with_options_and_arc_sink(
        &DiscoverOptions {
            allow_project_plugins: plugin_cfg.allow_project_plugins
                || env_flag_enabled("RUPOST_ALLOW_PROJECT_PLUGINS"),
            project_root: Some(working_dir.to_path_buf()),
        },
        &plugin_cfg,
        plugin_cfg.strict_vars,
        Arc::clone(&sink) as Arc<dyn WarningSink>,
    );
    (executor, plugin_cfg, sink)
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
    // B3：请求覆盖 > toml [plugins] > 默认（通过 WorkspaceConfig + BufferedWarningSink 单点加载）
    let (executor, plugin_cfg, sink) =
        build_web_executor(&state.working_dir, req.plugin_overrides.as_ref());
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

    // B3 strict 门禁：严格模式下降级形态的成功 → 判失败（与 CLI --strict-plugins 一致）
    let mut block_success = result.success;
    apply_strict_plugins_gate(
        &mut block_success,
        result.degraded,
        plugin_cfg.strict_plugins,
    );
    let mut warnings = sink.messages();
    for w in result.warnings {
        if !warnings.contains(&w) {
            warnings.push(w);
        }
    }
    let response = RunBlockResponse {
        success: block_success,
        status: result.status,
        duration_ms: result.duration.as_millis() as u64,
        assertions,
        timing_waterfall,
        response_headers,
        response_body,
        error: result.error,
        warnings,
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
    let mut parsed_file = match parse_document_by_path(&req.path, &content) {
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

    if let Some(sel) = req.selector.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let selector = crate::runner::selector::RequestSelector::Name(sel.to_string());
        if let Err(e) = crate::runner::selector::apply_selector(&mut parsed_file, &selector) {
            return json_err_session_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                e.to_string(),
                None,
            );
        }
    }

    // 3. 准备变量上下文（CLI 变量列表为空：环境级联与 .env 同 CLI 语义）
    let target_env = req.env.as_deref().or(state.default_env.as_deref());
    let (mut var_context, loader_warnings) = ConfigLoader::load_context(target_env, &[], None);
    for warning in &loader_warnings {
        tracing::warn!("web run: {warning}");
    }
    var_context.insert("__default_scheme", "http");
    if let Some(extra) = &req.extra_variables {
        for (k, v) in extra {
            var_context.insert(k, v);
        }
    }

    // 4. 内存隔离会话执行整套测试（ephemeral cookies，不污染本机会话；B-1：共享工厂装配插件注册表）。
    // R-4（免重填）+ WP-1（按浏览器会话隔离）：cookie 桥导入 + 捕获变量回灌。
    // B3：请求覆盖 > toml [plugins] > 默认（接入 WorkspaceConfig 与 BufferedWarningSink）
    let start_time = Instant::now();
    let (executor, plugin_cfg, sink) =
        build_web_executor(&state.working_dir, req.plugin_overrides.as_ref());
    for w in sink.messages() {
        tracing::warn!("web suite warning: {w}");
    }
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
        // B3 strict 门禁：严格模式下降级形态的成功 → 计入失败（与 CLI 一致）
        let mut success = r.success;
        apply_strict_plugins_gate(&mut success, r.degraded, plugin_cfg.strict_plugins);
        if r.skipped {
            skipped += 1;
        } else if success {
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
            source_line: r.source_line,
        });
    }

    // WP-R2：故障复现包（仅失败时；服务端管辖路径，客户端零路径输入；浏览器
    // 不是可信 secret 通道——secret_values 恒空，仅键名掩码，masking=secrets 仍强制）
    let mut incident_dir: Option<String> = None;
    if req.incident == Some(true) && failed > 0 {
        let bundle_name = format!(
            "{}-{}",
            Utc::now().format("%Y%m%dT%H%M%S"),
            &Uuid::new_v4().simple().to_string()[..8],
        );
        let target = state.incident_root.join(&bundle_name);
        let rel_display = normalize_posix_path(Path::new(".rupost/incidents").join(&bundle_name));
        let batch = vec![(PathBuf::from(&req.path), results)];
        let resolved_paths = vec![req.path.clone()];
        let env_owned = target_env.map(str::to_string);
        let incident_root_for_prune = state.incident_root.clone();

        let bundle_res = spawn_blocking(move || {
            // WP-R7：创建前裁剪最旧（32 上限；并发容差见常量注释）
            prune_incident_dirs(&incident_root_for_prune, MAX_INCIDENT_BUNDLES);
            create_incident_bundle(
                &batch,
                &resolved_paths,
                env_owned.as_deref(),
                &[],
                DEFAULT_MAX_SNAPSHOT_BODY_BYTES,
                &target,
            )
        })
        .await;

        match bundle_res {
            Ok(Ok(())) => {
                incident_dir = Some(rel_display);
            }
            Ok(Err(e)) => {
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
            Err(e) => {
                return json_err_session_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Incident bundle task failed: {e}"),
                    Some(&handle),
                );
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 命名判别表驱动：长度/字段字符类/分隔符（审查 E2 修正：尾段只认 hex）
    #[test]
    fn test_is_server_named_incident_table() {
        let cases = [
            ("20260101T000000-00000000", true),
            ("20260101T000000-0a1b2c3d", true),
            ("20260101T000000-0000000", false),  // len 23
            ("20260101t000000-00000000", false), // 小写 t
            ("20260101T000000-0000000g", false), // g 非 hex
            ("20260101000000-00000000", false),  // 缺 T
            ("20260101T0000", false),            // 过短
        ];
        for (input, expected) in cases {
            assert_eq!(is_server_named_incident(input), expected, "{input}");
        }
    }

    /// 裁剪：名称升序删最旧至 max-1（新 bundle 落位后稳态 = max）；
    /// 非命名目录与文件/symlink 不触碰
    #[test]
    fn test_prune_incident_dirs_keeps_non_named() {
        let dir = tempfile::TempDir::new().unwrap();
        for i in 0..3u32 {
            let d = dir.path().join(format!("20260101T000000-{i:08x}"));
            std::fs::create_dir(&d).unwrap();
            std::fs::write(d.join("s.txt"), "old").unwrap();
        }
        let keepme = dir.path().join("keepme");
        std::fs::create_dir(&keepme).unwrap();
        let plain_file = dir.path().join("20260101T000000-000000ff");
        std::fs::write(&plain_file, b"file not dir").unwrap();

        prune_incident_dirs(dir.path(), 2);

        assert!(
            !dir.path().join("20260101T000000-00000000").exists(),
            "最旧被删"
        );
        assert!(
            !dir.path().join("20260101T000000-00000001").exists(),
            "次旧被删"
        );
        assert!(
            dir.path()
                .join("20260101T000000-00000002")
                .join("s.txt")
                .is_file()
        );
        assert!(keepme.is_dir(), "非命名目录不触碰");
        assert!(plain_file.is_file(), "同名文件不触碰");
    }
}
