use crate::parser::MarkdownFileParser;
use crate::web::server::AppState;
use crate::web::types::{ApiResponse, SaveFileRequest, SaveFileResponse};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

pub async fn save_file_handler(
    State(state): State<AppState>,
    Json(req): Json<SaveFileRequest>,
) -> impl IntoResponse {
    // 1. 只读保护防线
    if state.readonly {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::err("Server is running in read-only mode")),
        );
    }

    // 2. 预写入语法校验防线 (AST Pre-write Syntax Guard)
    let parsed = match MarkdownFileParser::parse_content(&req.content) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ApiResponse::err(format!(
                    "Syntax guard rejection: Failed to parse markdown/http content: {}",
                    e
                ))),
            );
        }
    };

    let verified_blocks = parsed.requests.len();
    let bytes_written = req.content.len();

    // 3. 落盘写入
    match state.store.save_document(&req.path, &req.content).await {
        Ok(_) => (
            StatusCode::OK,
            Json(ApiResponse::ok(SaveFileResponse {
                path: req.path,
                bytes_written,
                verified_blocks,
            })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::err(format!("Failed to save document: {}", e))),
        ),
    }
}
