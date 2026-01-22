use std::time::Duration;

use crate::Result;
use crate::http::request::Request;
use crate::http::response::Response;
use crate::http::types::Method;

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
    pub fn new() -> Self {
        Self {
            inner: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("Failed to build HTTP client"),
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
        let duration = start.elapsed();

        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = response.text().await?;

        Response::new(status, headers, body, duration)
    }
}
