use crate::Result;
use crate::error::RupostError;
use crate::mock::matcher::{MockMatcher, MockRequest};
use crate::mock::server::MockServer;
use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use std::collections::HashMap;
use std::sync::Arc;

use colored::Colorize;

pub struct AxumMockServer;

impl MockServer for AxumMockServer {
    async fn start(
        &self,
        port: u16,
        matcher: Arc<dyn MockMatcher>,
        enable_cors: bool,
    ) -> Result<()> {
        let addr = format!("127.0.0.1:{}", port);
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(RupostError::IoError)?;

        serve_matcher(listener, matcher, enable_cors).await
    }
}

/// M3：智能 CORS 中间件——预检 OPTIONS 秒回 204；动态回显 Origin；
/// Allow-Headers 回显预检请求头（评审 P1-12：`*` 在 credentialed 请求下不展开）；
/// Credentials: true 仅在有 Origin 时携带（否则 `*` 与 credentials 组合违规）。
pub async fn cors_middleware(
    axum::extract::State(enable_cors): axum::extract::State<bool>,
    req: Request,
    next: axum::middleware::Next,
) -> Response {
    if !enable_cors {
        return next.run(req).await;
    }
    let origin = req.headers().get("origin").cloned();
    // 预检的 Access-Control-Request-Headers 在【请求头】上（评审 P1-12 回显来源）
    let request_headers = req.headers().get("access-control-request-headers").cloned();
    let is_options = req.method() == axum::http::Method::OPTIONS;

    let mut resp = if is_options {
        StatusCode::NO_CONTENT.into_response()
    } else {
        next.run(req).await
    };

    inject_cors_headers(
        resp.headers_mut(),
        origin.as_ref(),
        request_headers.as_ref(),
    );
    resp.headers_mut()
        .insert(axum::http::header::VARY, HeaderValue::from_static("Origin"));
    resp
}

fn inject_cors_headers(
    headers: &mut axum::http::HeaderMap,
    origin: Option<&HeaderValue>,
    request_headers: Option<&HeaderValue>,
) {
    if let Some(orig) = origin {
        headers.insert("Access-Control-Allow-Origin", orig.clone());
        headers.insert(
            "Access-Control-Allow-Credentials",
            HeaderValue::from_static("true"),
        );
    } else {
        headers.insert("Access-Control-Allow-Origin", HeaderValue::from_static("*"));
    }
    headers.insert(
        "Access-Control-Allow-Methods",
        HeaderValue::from_static("GET, POST, PUT, DELETE, PATCH, OPTIONS, HEAD"),
    );
    // 预检时回显 Access-Control-Request-Headers（* 与 credentials 组合被 Fetch 规范拒绝）
    if let Some(req_headers) = request_headers {
        headers.insert("Access-Control-Allow-Headers", req_headers.clone());
    }
}

/// 以已绑定的 listener 服务 mock 路由（TUI/编程接入：调用方持有端口信息）
pub async fn serve_matcher(
    listener: tokio::net::TcpListener,
    matcher: std::sync::Arc<dyn MockMatcher>,
    enable_cors: bool,
) -> Result<()> {
    let app = Router::new()
        .fallback(any(handle_mock_request))
        .with_state(matcher)
        .layer(axum::middleware::from_fn_with_state(
            enable_cors,
            cors_middleware,
        ));

    axum::serve(listener, app)
        .await
        .map_err(|e| RupostError::RequestExecutionFailed(e.to_string()))?;

    Ok(())
}

async fn handle_mock_request(
    State(matcher): State<Arc<dyn MockMatcher>>,
    req: Request,
) -> Response {
    let method = req.method().to_string();
    let path = req.uri().path().to_string();

    println!(
        "{} {} {}",
        "  [Incoming]".bold().blue(),
        method.bold().green(),
        path.cyan()
    );

    let query_str = req.uri().query().unwrap_or("");
    let query: HashMap<String, String> = url::form_urlencoded::parse(query_str.as_bytes())
        .into_owned()
        .collect();

    let mut headers = HashMap::new();
    for (name, value) in req.headers() {
        if let Ok(val_str) = value.to_str() {
            headers.insert(name.to_string(), val_str.to_string());
        }
    }

    let body_bytes = match axum::body::to_bytes(req.into_body(), 10 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => {
            println!(
                "  {} {}",
                "Status:".bold().white(),
                "400 Bad Request".bold().red()
            );
            return (StatusCode::BAD_REQUEST, "Failed to read request body").into_response();
        }
    };
    let body = String::from_utf8_lossy(&body_bytes).into_owned();

    let mock_req = MockRequest {
        method,
        path,
        headers,
        query,
        body,
    };

    if let Some(mock_resp) = matcher.match_request(&mock_req) {
        let status = match StatusCode::from_u16(mock_resp.status) {
            Ok(s) => s,
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };

        let matched_pattern = mock_resp
            .matched_pattern
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        println!(
            "  {} {} | {} {} | {}",
            "Status:".bold().white(),
            format!("{}", status.as_u16()).bold().green(),
            "Matched Route:".bold().white(),
            matched_pattern.cyan(),
            "Success".bold().green()
        );

        let is_sse = mock_resp.headers.iter().any(|(k, v)| {
            k.eq_ignore_ascii_case("content-type") && v.contains("text/event-stream")
        });

        let mut response = status.into_response();

        let headers_mut = response.headers_mut();
        for (k, v) in &mock_resp.headers {
            if let (Ok(h_name), Ok(h_val)) = (
                HeaderName::from_bytes(k.as_bytes()),
                HeaderValue::from_str(v),
            ) {
                headers_mut.insert(h_name, h_val);
            }
        }

        if is_sse {
            use futures_util::stream;
            use std::convert::Infallible;
            use std::time::Duration;

            let chunks: Vec<String> = mock_resp
                .body
                .split('\n')
                .map(|s| format!("{}\n", s))
                .collect();

            let stream = stream::unfold((chunks, 0), |(chunks, index)| async move {
                if index >= chunks.len() {
                    None
                } else {
                    if index > 0 {
                        tokio::time::sleep(Duration::from_millis(40)).await;
                    }
                    let chunk = chunks[index].clone();
                    Some((
                        Ok::<_, Infallible>(axum::body::Bytes::from(chunk)),
                        (chunks, index + 1),
                    ))
                }
            });

            *response.body_mut() = Body::from_stream(stream);
        } else {
            *response.body_mut() = Body::from(mock_resp.body);
        }

        response
    } else {
        println!(
            "  {} {} | {}",
            "Status:".bold().white(),
            "404 Not Found".bold().red(),
            "No Matching Mock Route Found".bold().red()
        );

        let error_msg = format!(
            r#"{{"error": "No matching mock route found", "request": {{"method": "{}", "path": "{}"}}}}"#,
            mock_req.method, mock_req.path
        );
        (
            StatusCode::NOT_FOUND,
            [("Content-Type", "application/json")],
            error_msg,
        )
            .into_response()
    }
}
