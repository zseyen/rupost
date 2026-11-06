use crate::web::importer::MultiFormatImporter;
use crate::web::server::AppState;
use crate::web::types::{ApiResponse, ImportFormat, ImportRequest};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;

pub async fn import_handler(
    State(_state): State<AppState>,
    Json(req): Json<ImportRequest>,
) -> impl IntoResponse {
    match MultiFormatImporter::import(&req) {
        Ok(resp) => (StatusCode::OK, Json(ApiResponse::ok(resp))),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::err(format!("Import failed: {}", e))),
        ),
    }
}

pub async fn import_curl_handler(
    State(state): State<AppState>,
    Json(mut req): Json<ImportRequest>,
) -> impl IntoResponse {
    req.format = ImportFormat::Curl;
    import_handler(State(state), Json(req)).await
}
