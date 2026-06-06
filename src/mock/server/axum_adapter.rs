use std::collections::HashMap;
use std::sync::Arc;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::Router;
use crate::mock::matcher::{MockMatcher, MockRequest};
use crate::mock::server::MockServer;
use crate::Result;
use crate::error::RupostError;

use colored::Colorize;

pub struct AxumMockServer;

impl MockServer for AxumMockServer {
    async fn start(&self, port: u16, matcher: Arc<dyn MockMatcher>) -> Result<()> {
        let app = Router::new()
            .fallback(any(handle_mock_request))
            .with_state(matcher);

        let addr = format!("127.0.0.1:{}", port);
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(RupostError::IoError)?;

        axum::serve(listener, app)
            .await
            .map_err(|e| RupostError::RequestExecutionFailed(e.to_string()))?;

        Ok(())
    }
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
            println!("  {} {}", "Status:".bold().white(), "400 Bad Request".bold().red());
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

        let matched_pattern = mock_resp.matched_pattern.clone().unwrap_or_else(|| "unknown".to_string());
        println!(
            "  {} {} | {} {} | {}",
            "Status:".bold().white(),
            format!("{}", status.as_u16()).bold().green(),
            "Matched Route:".bold().white(),
            matched_pattern.cyan(),
            "Success".bold().green()
        );

        let mut response = status.into_response();
        
        let headers_mut = response.headers_mut();
        for (k, v) in mock_resp.headers {
            if let (Ok(h_name), Ok(h_val)) = (
                HeaderName::from_bytes(k.as_bytes()),
                HeaderValue::from_str(&v),
            ) {
                headers_mut.insert(h_name, h_val);
            }
        }

        *response.body_mut() = Body::from(mock_resp.body);
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
