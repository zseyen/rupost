use std::sync::Arc;
use std::time::Duration;

use crate::Result;
use crate::http::request::Request;
use crate::http::response::Response;
use crate::http::types::Method;
use reqwest_cookie_store::CookieStoreMutex;

#[derive(Clone)]
pub struct Client {
    inner: reqwest::Client,
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

impl Client {
    /// Create a new client without cookie support.
    pub fn new() -> Self {
        Self {
            inner: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("Failed to build HTTP client"),
        }
    }

    /// Create a new client with cookie store support.
    ///
    /// # Arguments
    /// * `cookie_store` - The cookie store to use for automatic cookie handling.
    pub fn with_cookie_store(cookie_store: Arc<CookieStoreMutex>) -> Self {
        Self {
            inner: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .cookie_store(true)
                .cookie_provider(cookie_store)
                .build()
                .expect("Failed to build HTTP client with cookie store"),
        }
    }

    pub async fn execute(&self, request: Request) -> Result<Response> {
        let url = reqwest::Url::parse_with_params(&request.url.to_string(), &request.query_params)?;
        let method = match request.method {
            Method::Get => reqwest::Method::GET,
            Method::Post => reqwest::Method::POST,
            Method::Put => reqwest::Method::PUT,
            Method::Delete => reqwest::Method::DELETE,
            Method::Patch => reqwest::Method::PATCH,
            Method::Head => reqwest::Method::HEAD,
            Method::Options => reqwest::Method::OPTIONS,
        };
        let mut req = self.inner.request(method, url).headers(request.headers);

        if let Some(body) = request.body {
            req = req.body(body);
        }

        let start = std::time::Instant::now();
        let response = req.send().await?;
        let ttfb = start.elapsed();

        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = response.text().await?;
        let total_duration = start.elapsed();
        let transfer = total_duration.saturating_sub(ttfb);

        Response::new(status, headers, body, total_duration, ttfb, transfer)
    }
}
