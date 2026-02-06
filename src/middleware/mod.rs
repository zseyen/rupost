//! Middleware infrastructure for RuPost.
//!
//! Provides hooks for intercepting requests before execution
//! and responses after execution.

mod cookie;

pub use cookie::{CookieMiddleware, CookieMode};

use crate::Result;
use crate::http::request::Request;
use crate::http::response::Response;
use async_trait::async_trait;

/// Middleware trait for intercepting HTTP requests and responses.
#[async_trait]
pub trait Middleware: Send + Sync {
    /// Called before an HTTP request is executed.
    /// Can modify the request (e.g., add headers, cookies).
    async fn before_request(&self, req: &mut Request) -> Result<()>;

    /// Called after an HTTP response is received.
    /// Can process the response (e.g., extract cookies).
    async fn after_response(&self, resp: &Response) -> Result<()>;
}
