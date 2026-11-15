use std::sync::Arc;
use std::time::Duration;

use crate::Result;
use crate::http::request::Request;
use crate::http::response::Response;
use crate::http::timing_connector::{
    ACTIVE_TIMING_SLOT, ConnectSpan, RequestTimingSlot, TimingConnectorLayer, TimingDnsResolver,
};
use crate::http::types::Method;
use reqwest_cookie_store::CookieStoreMutex;

#[derive(Clone)]
pub struct Client {
    inner: reqwest::Client,
}

impl Default for Client {
    fn default() -> Self {
        Self::new(None)
    }
}

impl Client {
    fn base_builder(ua: &str) -> reqwest::ClientBuilder {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(ua)
            .dns_resolver(Arc::new(TimingDnsResolver))
            .connector_layer(TimingConnectorLayer)
    }

    /// Create a new client without cookie support.
    pub fn new(user_agent: Option<&str>) -> Self {
        let ua = user_agent.unwrap_or("rupost/1.0.0");
        Self {
            inner: Self::base_builder(ua)
                .build()
                .expect("Failed to build HTTP client"),
        }
    }

    /// Create a new client with cookie store support.
    ///
    /// # Arguments
    /// * `cookie_store` - The cookie store to use for automatic cookie handling.
    pub fn with_cookie_store(
        cookie_store: Arc<CookieStoreMutex>,
        user_agent: Option<&str>,
    ) -> Self {
        let ua = user_agent.unwrap_or("rupost/1.0.0");
        Self {
            inner: Self::base_builder(ua)
                .cookie_store(true)
                .cookie_provider(cookie_store)
                .build()
                .expect("Failed to build HTTP client with cookie store"),
        }
    }

    fn build_reqwest(&self, request: Request) -> Result<reqwest::RequestBuilder> {
        let url = if request.query_params.is_empty() {
            reqwest::Url::parse(&request.url.to_string())?
        } else {
            reqwest::Url::parse_with_params(&request.url.to_string(), &request.query_params)?
        };
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

        if let Some(timeout) = request.timeout {
            req = req.timeout(timeout);
        }

        if let Some(body) = request.body {
            req = req.body(body);
        }

        Ok(req)
    }

    /// 执行请求并捕获单连接时序观测区间（`ConnectSpan`）。
    /// 连接池复用时 `ConnectSpan` 为 `None`。
    pub async fn execute_raw_with_timing(
        &self,
        request: Request,
    ) -> Result<(reqwest::Response, Duration, Option<ConnectSpan>)> {
        let req = self.build_reqwest(request)?;
        let slot = Arc::new(RequestTimingSlot::default());
        let start = std::time::Instant::now();
        let response = ACTIVE_TIMING_SLOT
            .scope(slot.clone(), async { req.send().await })
            .await?;
        let ttfb = start.elapsed();
        let span = slot.take_span();
        Ok((response, ttfb, span))
    }

    pub async fn execute_raw(&self, request: Request) -> Result<(reqwest::Response, Duration)> {
        let (response, ttfb, _) = self.execute_raw_with_timing(request).await?;
        Ok((response, ttfb))
    }

    pub async fn execute(&self, request: Request) -> Result<Response> {
        let start = std::time::Instant::now();
        let (response, ttfb) = self.execute_raw(request).await?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = response.text().await?;
        let total_duration = start.elapsed();
        let transfer = total_duration.saturating_sub(ttfb);

        Response::new(status, headers, body, total_duration, ttfb, transfer)
    }
}
