use std::sync::Arc;
use crate::mock::matcher::MockMatcher;
use crate::Result;

#[allow(async_fn_in_trait)]
pub trait MockServer: Send + Sync {
    async fn start(&self, port: u16, matcher: Arc<dyn MockMatcher>) -> Result<()>;
}
