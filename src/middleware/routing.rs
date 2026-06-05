use std::collections::HashMap;
use crate::Result;
use crate::middleware::Middleware;
use crate::http::Request;
use crate::http::Response;

#[derive(Debug, Clone)]
pub struct RoutingRule {
    pub match_host: String,
    pub replace_host: String,
    pub inject_headers: HashMap<String, String>,
}

pub struct RoutingMiddleware {
    pub rules: Vec<RoutingRule>,
}

impl RoutingMiddleware {
    pub fn new(rules: Vec<RoutingRule>) -> Self {
        Self { rules }
    }
}

impl Middleware for RoutingMiddleware {
    async fn before_request(&self, _req: &mut Request) -> Result<()> {
        // MVP 骨架暂时不修改请求
        Ok(())
    }

    async fn after_response(&self, _resp: &Response) -> Result<()> {
        Ok(())
    }
}
