//! Middleware infrastructure for RuPost.
//!
//! Provides hooks for intercepting requests before execution
//! and responses after execution.

mod cookie;

pub use cookie::{CookieMiddleware, CookieMode, resolve_cookie_path};
use std::future::Future;

use crate::Result;
use crate::http::request::Request;
use crate::http::response::Response;

/// Middleware trait for intercepting HTTP requests and responses.
pub trait Middleware: Send + Sync {
    /// Called before an HTTP request is executed.
    /// Can modify the request (e.g., add headers, cookies).
    fn before_request(&self, req: &mut Request) -> impl Future<Output = Result<()>> + Send;

    /// Called after an HTTP response is received.
    /// Can process the response (e.g., extract cookies).
    fn after_response(&self, resp: &Response) -> impl Future<Output = Result<()>> + Send;
}
