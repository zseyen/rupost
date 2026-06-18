use crate::Result;
use crate::mock::matcher::MockMatcher;
use std::sync::Arc;

#[allow(async_fn_in_trait)]
pub trait MockServer: Send + Sync {
    async fn start(&self, port: u16, matcher: Arc<dyn MockMatcher>) -> Result<()>;
}

pub struct DummyMockServer;

impl MockServer for DummyMockServer {
    async fn start(&self, port: u16, matcher: Arc<dyn MockMatcher>) -> Result<()> {
        let _ = port;
        let _ = matcher;
        Ok(())
    }
}

pub mod axum_adapter;
pub use axum_adapter::AxumMockServer;
