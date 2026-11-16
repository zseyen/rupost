use crate::web::markdown::render_markdown_to_html;
use crate::web::types::{ApiResponse, RenderMarkdownRequest};
use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;

/// 将 Markdown 源码渲染为安全的活文档 HTML（支持只读模式）
pub async fn render_markdown_handler(Json(req): Json<RenderMarkdownRequest>) -> impl IntoResponse {
    let rendered = render_markdown_to_html(&req.content);
    (StatusCode::OK, Json(ApiResponse::ok(rendered)))
}
