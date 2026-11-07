use crate::parser::parse_document_by_path;
use crate::web::server::AppState;
use crate::web::types::{ApiResponse, SaveFileRequest, SaveFileResponse};
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;

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

    // 2. 预写入语法校验防线：统一委托给 parse_document_by_path 门面组件
    let parsed = match parse_document_by_path(&req.path, &req.content) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ApiResponse::err(format!(
                    "Syntax guard rejection: Failed to parse document: {}",
                    e
                ))),
            );
        }
    };

    let verified_blocks = parsed.requests.len();
    let bytes_written = req.content.len();

    // 3. 落盘写入（存储层通过 PathSandbox 进行绝对路径、隐藏文件与扩展名安全拦截）
    match state.store.save_document(&req.path, &req.content).await {
        Ok(_) => (
            StatusCode::OK,
            Json(ApiResponse::ok(SaveFileResponse {
                path: req.path,
                bytes_written,
                verified_blocks,
            })),
        ),
        // 安全拦截（目录穿越/隐藏文件/扩展名白名单）精准映射 403，其余存储错误保持 400
        Err(e) => {
            let status = if matches!(e, crate::error::RupostError::SecurityViolation(_)) {
                StatusCode::FORBIDDEN
            } else {
                StatusCode::BAD_REQUEST
            };
            (
                status,
                Json(ApiResponse::err(format!("Failed to save document: {}", e))),
            )
        }
    }
}
