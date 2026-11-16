use crate::exporter::plugin::ExporterRegistry;
use crate::exporter::preparer::RequestPreparer;
use crate::exporter::types::ExportOptions;
use crate::parser::http_file::HttpFileParser;
use crate::variable::config::ConfigLoader;
use crate::web::server::AppState;
use crate::web::types::{ApiResponse, ExportBlockRequest, ExportBlockResponse};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use std::collections::HashSet;

/// 将单块 HTTP 请求导出为目标 CLI 命令（如 curl / httpie / wget）；无副作用，只读模式可用
pub async fn export_block_handler(
    State(state): State<AppState>,
    Json(req): Json<ExportBlockRequest>,
) -> impl IntoResponse {
    let format_name = req
        .format
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("curl")
        .to_lowercase();

    let registry = ExporterRegistry::new_builtin();
    let exporter = match registry.find(&format_name) {
        Some(exp) => exp,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::err(format!(
                    "Unsupported export format: {} (supported: curl, httpie, wget)",
                    format_name
                ))),
            );
        }
    };

    let parsed_file = match HttpFileParser::parse_content(&req.raw_block) {
        Ok(pf) => pf,
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

    let mut cli_vars = Vec::new();
    if let Some(extra) = &req.extra_variables {
        for (k, v) in extra {
            cli_vars.push((k.clone(), v.clone()));
        }
    }

    let target_env = req.env.as_deref().or(state.default_env.as_deref());
    let (mut var_context, mut warnings) = ConfigLoader::load_context(target_env, &cli_vars, None);
    var_context.insert("__default_scheme", "http");

    let redact_secrets = req.redact_secrets.unwrap_or(true);
    let captured_symbols = HashSet::new();
    let exportable = match RequestPreparer::prepare_one(
        target_request,
        &mut var_context,
        None,
        req.file_path.as_deref(),
        redact_secrets,
        &captured_symbols,
    ) {
        Ok(exp) => exp,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::err(format!(
                    "Failed to prepare request for export: {}",
                    e
                ))),
            );
        }
    };
    warnings.extend(exportable.warnings.iter().cloned());

    let opts = ExportOptions {
        compact: req.compact.unwrap_or(false),
        redact_secrets,
    };

    match exporter.export_one(&exportable, &opts) {
        Ok(command) => (
            StatusCode::OK,
            Json(ApiResponse::ok(ExportBlockResponse {
                format: exporter.name().to_string(),
                command,
                warnings,
            })),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::err(format!("Export failed: {}", e))),
        ),
    }
}
