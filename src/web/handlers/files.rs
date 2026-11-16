use crate::error::RupostError;
use crate::parser::parse_document_by_path;
use crate::variable::config::ConfigLoader;
use crate::web::http_parse::parse_http_document_to_cards;
use crate::web::server::AppState;
use crate::web::types::{
    ApiResponse, CreateFileRequest, CreateFileResponse, EnvsResponse, ParseHttpRequest,
    WorkbenchMeta,
};
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct ListFilesQuery {
    pub path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GetFileQuery {
    pub path: String,
}

pub async fn list_files_handler(
    State(state): State<AppState>,
    Query(query): Query<ListFilesQuery>,
) -> impl IntoResponse {
    match state.store.list_documents(query.path.as_deref()).await {
        Ok(entries) => (StatusCode::OK, Json(ApiResponse::ok(entries))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::err(e.to_string())),
        ),
    }
}

pub async fn get_file_handler(
    State(state): State<AppState>,
    Query(query): Query<GetFileQuery>,
) -> impl IntoResponse {
    match state.store.read_document(&query.path).await {
        Ok(content) => (StatusCode::OK, Json(ApiResponse::ok(content))),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::err(format!(
                "File not found or unreadable: {}",
                e
            ))),
        ),
    }
}

/// 工作台元信息：前端据此渲染只读徽章并置灰写路径按钮（UX-01）
pub async fn meta_handler(State(state): State<AppState>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(ApiResponse::ok(WorkbenchMeta {
            readonly: state.readonly,
        })),
    )
}

/// 新建活文档（只读模式拦截，沙箱校验，禁止覆盖已存在文件）
pub async fn create_file_handler(
    State(state): State<AppState>,
    Json(req): Json<CreateFileRequest>,
) -> impl IntoResponse {
    if state.readonly {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::err("Server is running in read-only mode")),
        );
    }

    let content = req.content.unwrap_or_default();
    if !content.trim().is_empty()
        && let Err(e) = parse_document_by_path(&req.path, &content)
    {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ApiResponse::err(format!(
                "Syntax guard rejection: Failed to parse document: {}",
                e
            ))),
        );
    }

    match state.store.create_document(&req.path, &content).await {
        Ok(bytes_written) => (
            StatusCode::CREATED,
            Json(ApiResponse::ok(CreateFileResponse {
                path: req.path,
                bytes_written,
            })),
        ),
        Err(e) => {
            let status = if matches!(e, RupostError::SecurityViolation(_)) {
                StatusCode::FORBIDDEN
            } else if e.to_string().contains("already exists") {
                StatusCode::CONFLICT
            } else {
                StatusCode::BAD_REQUEST
            };
            (
                status,
                Json(ApiResponse::err(format!(
                    "Failed to create document: {}",
                    e
                ))),
            )
        }
    }
}

/// 发现工作区可用环境名（来自 rupost.toml [environments.*] 与 .env.<name> 文件；不泄露任何密钥值）
pub fn discover_workspace_envs(working_dir: &Path) -> Vec<String> {
    let mut names = BTreeSet::new();

    let toml_path = working_dir.join("rupost.toml");
    if toml_path.is_file()
        && let Ok(cfg) = ConfigLoader::load_from_path(&toml_path)
    {
        for k in cfg.environments.keys() {
            names.insert(k.clone());
        }
    }

    if let Ok(entries) = std::fs::read_dir(working_dir) {
        for entry in entries.flatten() {
            let file_name = entry.file_name().to_string_lossy().to_string();
            if let Some(env_name) = file_name.strip_prefix(".env.")
                && !env_name.is_empty()
                && !matches!(env_name, "example" | "sample" | "template")
                && entry.path().is_file()
            {
                names.insert(env_name.to_string());
            }
        }
    }

    names.into_iter().collect()
}

/// 列出工作区可用环境名及默认环境
pub async fn list_envs_handler(State(state): State<AppState>) -> impl IntoResponse {
    let environments = discover_workspace_envs(&state.working_dir);
    (
        StatusCode::OK,
        Json(ApiResponse::ok(EnvsResponse {
            environments,
            default_env: state.default_env.clone(),
        })),
    )
}

/// 使用权威 `HttpFileParser` 解析 `.http` 内容为结构化卡片（只读模式可用）
pub async fn parse_http_handler(Json(req): Json<ParseHttpRequest>) -> impl IntoResponse {
    match parse_http_document_to_cards(&req.content) {
        Ok(parsed) => (StatusCode::OK, Json(ApiResponse::ok(parsed))),
        Err(err) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ApiResponse::err(err)),
        ),
    }
}
