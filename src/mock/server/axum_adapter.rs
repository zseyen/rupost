use crate::Result;
use crate::error::RupostError;
use crate::mock::matcher::{MockMatcher, MockRequest};
use crate::mock::server::MockServer;
use axum::Router;
use axum::body::{Body, Bytes, to_bytes};
use axum::extract::{Request, State};
use axum::http::header::VARY;
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use futures_util::stream;
use std::collections::HashMap;
use std::convert::Infallible;
use std::future::{Future, pending};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::mpsc::UnboundedSender;
use tokio::time::sleep;

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
        let listener = TcpListener::bind(&addr)
            .await
            .map_err(RupostError::IoError)?;

        serve_matcher_inner(listener, matcher, enable_cors, true, None, pending()).await
    }
}

/// Mock 流量观测事件：供 CLI / TUI / Web 三端统一消费
#[derive(Debug, Clone)]
pub struct MockTrafficEvent {
    pub method: String,
    pub path: String,
    pub status: u16,
    pub matched_pattern: Option<String>,
    pub elapsed_ms: u64,
}

#[derive(Clone)]
struct MockServerState {
    matcher: Arc<dyn MockMatcher>,
    log_to_stdout: bool,
    event_tx: Option<UnboundedSender<MockTrafficEvent>>,
}

/// M3：智能 CORS 中间件——预检 OPTIONS 秒回 204；动态回显 Origin；
/// Allow-Headers 回显预检请求头（评审 P1-12：`*` 在 credentialed 请求下不展开）；
/// Credentials: true 仅在有 Origin 时携带（否则 `*` 与 credentials 组合违规）。
pub async fn cors_middleware(
    State(enable_cors): State<bool>,
    req: Request,
    next: Next,
) -> Response {
    if !enable_cors {
        return next.run(req).await;
    }
    let origin = req.headers().get("origin").cloned();
    // 预检的 Access-Control-Request-Headers 在【请求头】上（评审 P1-12 回显来源）
    let request_headers = req.headers().get("access-control-request-headers").cloned();
    let is_options = req.method() == Method::OPTIONS;

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
        .insert(VARY, HeaderValue::from_static("Origin"));
    resp
}

fn inject_cors_headers(
    headers: &mut HeaderMap,
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
    listener: TcpListener,
    matcher: Arc<dyn MockMatcher>,
    enable_cors: bool,
) -> Result<()> {
    serve_matcher_with_shutdown(listener, matcher, enable_cors, pending()).await
}

/// 以已绑定的 listener 服务 mock 路由，支持优雅关闭信号（Web 工作台托管用）
pub async fn serve_matcher_with_shutdown<F>(
    listener: TcpListener,
    matcher: Arc<dyn MockMatcher>,
    enable_cors: bool,
    shutdown: F,
) -> Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    serve_matcher_inner(listener, matcher, enable_cors, false, None, shutdown).await
}

/// 以已绑定的 listener 服务 mock 路由，支持优雅关闭与流量观测事件流
pub async fn serve_matcher_with_events<F>(
    listener: TcpListener,
    matcher: Arc<dyn MockMatcher>,
    enable_cors: bool,
    shutdown: F,
    event_tx: Option<UnboundedSender<MockTrafficEvent>>,
) -> Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    serve_matcher_inner(listener, matcher, enable_cors, false, event_tx, shutdown).await
}

async fn serve_matcher_inner<F>(
    listener: TcpListener,
    matcher: Arc<dyn MockMatcher>,
    enable_cors: bool,
    log_to_stdout: bool,
    event_tx: Option<UnboundedSender<MockTrafficEvent>>,
    shutdown: F,
) -> Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    let state = MockServerState {
        matcher,
        log_to_stdout,
        event_tx,
    };
    let app = Router::new()
        .fallback(any(handle_mock_request))
        .with_state(state)
        .layer(middleware::from_fn_with_state(enable_cors, cors_middleware));

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
        .map_err(|e| RupostError::RequestExecutionFailed(e.to_string()))?;

    Ok(())
}

fn emit_traffic_event(
    state: &MockServerState,
    method: &str,
    path: &str,
    status: u16,
    matched_pattern: Option<String>,
    started: Instant,
) {
    if state.log_to_stdout {
        if let Some(ref pattern) = matched_pattern {
            println!(
                "  {} {} | {} {} | {}",
                "Status:".bold().white(),
                format!("{status}").bold().green(),
                "Matched Route:".bold().white(),
                pattern.cyan(),
                "Success".bold().green()
            );
        } else if status == 404 {
            println!(
                "  {} {} | {}",
                "Status:".bold().white(),
                "404 Not Found".bold().red(),
                "No Matching Mock Route Found".bold().red()
            );
        } else {
            println!(
                "  {} {}",
                "Status:".bold().white(),
                format!("{status} Bad Request").bold().red()
            );
        }
    } else {
        tracing::debug!(
            method = %method,
            path = %path,
            status = status,
            matched_pattern = ?matched_pattern,
            "mock request handled"
        );
    }

    if let Some(ref tx) = state.event_tx {
        let _ = tx.send(MockTrafficEvent {
            method: method.to_string(),
            path: path.to_string(),
            status,
            matched_pattern,
            elapsed_ms: started.elapsed().as_millis() as u64,
        });
    }
}

async fn handle_mock_request(State(state): State<MockServerState>, req: Request) -> Response {
    let started = Instant::now();
    let matcher = &state.matcher;
    let method = req.method().to_string();
    let path = req.uri().path().to_string();

    if state.log_to_stdout {
        println!(
            "{} {} {}",
            "  [Incoming]".bold().blue(),
            method.bold().green(),
            path.cyan()
        );
    }

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

    let body_bytes = match to_bytes(req.into_body(), 10 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => {
            emit_traffic_event(&state, &method, &path, 400, None, started);
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

        emit_traffic_event(
            &state,
            &mock_req.method,
            &mock_req.path,
            status.as_u16(),
            mock_resp.matched_pattern.clone(),
            started,
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
            let chunks = split_sse_frames(&mock_resp.body);

            let stream = stream::unfold((chunks, 0), |(chunks, index)| async move {
                if index >= chunks.len() {
                    None
                } else {
                    if index > 0 {
                        sleep(Duration::from_millis(40)).await;
                    }
                    let chunk = chunks[index].clone();
                    Some((Ok::<_, Infallible>(Bytes::from(chunk)), (chunks, index + 1)))
                }
            });

            *response.body_mut() = Body::from_stream(stream);
        } else {
            *response.body_mut() = Body::from(mock_resp.body);
        }

        response
    } else {
        emit_traffic_event(&state, &mock_req.method, &mock_req.path, 404, None, started);
        not_found_response(&mock_req.method, &mock_req.path)
    }
}

/// SSE 事件帧切分：按空行（\n\n 或 \r\n\r\n）界定事件边界，保留多行事件完整性
fn split_sse_frames(body: &str) -> Vec<String> {
    let normalized = body.replace("\r\n", "\n");
    normalized
        .split("\n\n")
        .filter(|frame| !frame.trim().is_empty())
        .map(|frame| format!("{frame}\n\n"))
        .collect()
}

/// 使用 serde_json 安全构造 404 响应体，避免路径含特殊字符破坏 JSON 结构
fn not_found_response(method: &str, path: &str) -> Response {
    let payload = serde_json::json!({
        "error": "No matching mock route found",
        "request": {
            "method": method,
            "path": path,
        }
    });
    (StatusCode::NOT_FOUND, axum::Json(payload)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::matcher::TrieRouteMatcher;
    use crate::mock::variant::MockVariant;
    use reqwest::Client;
    use tokio::sync::{mpsc, oneshot};

    #[test]
    fn test_split_sse_frames_preserves_multiline_events() {
        let raw = "event: delta\nid: 1\ndata: {\"a\":1}\n\nevent: done\ndata: [DONE]\n\n";
        let frames = split_sse_frames(raw);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0], "event: delta\nid: 1\ndata: {\"a\":1}\n\n");
        assert_eq!(frames[1], "event: done\ndata: [DONE]\n\n");
    }

    #[tokio::test]
    async fn test_not_found_response_escapes_special_chars() {
        let resp = not_found_response("GET", "/bad\"path\\test");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let bytes = to_bytes(resp.into_body(), 4096).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed["request"]["path"], "/bad\"path\\test");
    }

    #[tokio::test]
    async fn test_serve_matcher_with_events_emits_traffic_events() {
        let mut matcher = TrieRouteMatcher::new();
        matcher.add_route(
            "GET",
            "/api/hello",
            vec![MockVariant {
                condition: None,
                status: 200,
                headers: HashMap::new(),
                response_body: "hi".to_string(),
            }],
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        let handle = tokio::spawn(async move {
            let _ = serve_matcher_with_events(
                listener,
                Arc::new(matcher),
                false,
                async move {
                    let _ = shutdown_rx.await;
                },
                Some(tx),
            )
            .await;
        });

        let client = Client::new();
        let _ = client
            .get(format!("http://127.0.0.1:{port}/api/hello"))
            .send()
            .await
            .unwrap();
        let _ = client
            .get(format!("http://127.0.0.1:{port}/api/missing"))
            .send()
            .await
            .unwrap();

        let ev1 = rx.recv().await.expect("first event");
        assert_eq!(ev1.method, "GET");
        assert_eq!(ev1.path, "/api/hello");
        assert_eq!(ev1.status, 200);
        assert_eq!(ev1.matched_pattern.as_deref(), Some("/api/hello"));

        let ev2 = rx.recv().await.expect("second event");
        assert_eq!(ev2.status, 404);
        assert!(ev2.matched_pattern.is_none());

        let _ = shutdown_tx.send(());
        let _ = handle.await;
    }
}
