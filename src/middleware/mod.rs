//! Middleware infrastructure for RuPost.
//!
//! Provides hooks for intercepting requests before execution
//! and responses after execution.

mod cookie;
pub mod routing;

pub use cookie::{CookieMiddleware, CookieMode, ERR_COOKIE_PERSIST, resolve_cookie_path};
pub use routing::{RoutingMiddleware, RoutingRule};

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::Result;
use crate::http::request::Request;
use crate::http::response::Response;

/// Boxed future type for dyn-compatible async middleware hooks.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Middleware trait for intercepting HTTP requests and responses.
pub trait Middleware: Send + Sync {
    /// Called before an HTTP request is executed.
    /// Can modify the request (e.g., add headers, cookies).
    fn before_request<'a>(&'a self, req: &'a mut Request) -> BoxFuture<'a, Result<()>>;

    /// Called after an HTTP response is received.
    /// Can process the response (e.g., extract cookies).
    fn after_response<'a>(&'a self, resp: &'a Response) -> BoxFuture<'a, Result<()>>;
}

/// Ordered pipeline of dyn-compatible middleware layers.
#[derive(Clone, Default)]
pub struct MiddlewarePipeline {
    layers: Vec<Arc<dyn Middleware>>,
}

impl MiddlewarePipeline {
    /// Create an empty middleware pipeline.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a concrete middleware layer.
    pub fn push<M: Middleware + 'static>(&mut self, layer: M) {
        self.layers.push(Arc::new(layer));
    }

    /// Append an already-shared middleware layer.
    pub fn push_arc(&mut self, layer: Arc<dyn Middleware>) {
        self.layers.push(layer);
    }

    /// Number of layers in the pipeline.
    pub fn len(&self) -> usize {
        self.layers.len()
    }

    /// Whether the pipeline has no layers.
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    /// Execute `before_request` hooks in insertion order, short-circuiting on error.
    pub async fn run_before_request(&self, req: &mut Request) -> Result<()> {
        for layer in &self.layers {
            layer.before_request(req).await?;
        }
        Ok(())
    }

    /// Execute `after_response` hooks in insertion order, short-circuiting on error.
    pub async fn run_after_response(&self, resp: &Response) -> Result<()> {
        for layer in &self.layers {
            layer.after_response(resp).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RupostError;
    use std::sync::Mutex;
    use std::time::Duration;

    struct RecordingMiddleware {
        name: &'static str,
        log: Arc<Mutex<Vec<String>>>,
        fail_before: bool,
    }

    impl Middleware for RecordingMiddleware {
        fn before_request<'a>(&'a self, _req: &'a mut Request) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                self.log
                    .lock()
                    .unwrap()
                    .push(format!("before:{}", self.name));
                if self.fail_before {
                    return Err(RupostError::Other(format!("failed:{}", self.name)));
                }
                Ok(())
            })
        }

        fn after_response<'a>(&'a self, _resp: &'a Response) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                self.log
                    .lock()
                    .unwrap()
                    .push(format!("after:{}", self.name));
                Ok(())
            })
        }
    }

    fn dummy_request() -> Request {
        Request::new("GET", "http://localhost/test").unwrap()
    }

    fn dummy_response() -> Response {
        Response::new(
            200,
            reqwest::header::HeaderMap::new(),
            String::new(),
            Duration::from_millis(1),
            Duration::from_millis(1),
            Duration::ZERO,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn test_pipeline_order_clone_and_short_circuit() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut pipeline = MiddlewarePipeline::new();
        assert!(pipeline.is_empty());

        pipeline.push(RecordingMiddleware {
            name: "first",
            log: log.clone(),
            fail_before: false,
        });
        pipeline.push_arc(Arc::new(RecordingMiddleware {
            name: "second",
            log: log.clone(),
            fail_before: false,
        }));
        assert_eq!(pipeline.len(), 2);

        let cloned = pipeline.clone();
        let mut req = dummy_request();
        cloned.run_before_request(&mut req).await.unwrap();
        let resp = dummy_response();
        cloned.run_after_response(&resp).await.unwrap();

        assert_eq!(
            *log.lock().unwrap(),
            vec![
                "before:first",
                "before:second",
                "after:first",
                "after:second"
            ]
        );

        // Short-circuit on before_request error
        log.lock().unwrap().clear();
        let mut failing = MiddlewarePipeline::new();
        failing.push(RecordingMiddleware {
            name: "a",
            log: log.clone(),
            fail_before: true,
        });
        failing.push(RecordingMiddleware {
            name: "b",
            log: log.clone(),
            fail_before: false,
        });
        let err = failing.run_before_request(&mut req).await.unwrap_err();
        assert!(err.to_string().contains("failed:a"));
        assert_eq!(*log.lock().unwrap(), vec!["before:a"]);
    }
}
