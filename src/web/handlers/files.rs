use crate::web::server::AppState;
use crate::web::types::ApiResponse;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

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
            Json(ApiResponse::err(format!("File not found or unreadable: {}", e))),
        ),
    }
}
