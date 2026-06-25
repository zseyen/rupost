use crate::Result;
use crate::http::Request;
use crate::http::Response;
use crate::middleware::Middleware;
use std::collections::HashMap;

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
    async fn before_request(&self, req: &mut Request) -> Result<()> {
        let current_host = req.url.host.clone();
        for rule in &self.rules {
            if current_host == rule.match_host {
                // Parse replace_host into host and port
                let parts: Vec<&str> = rule.replace_host.split(':').collect();
                if parts.len() == 2 {
                    req.url.host = parts[0].to_string();
                    if let Ok(p) = parts[1].parse::<u16>() {
                        req.url.port = p;
                    }
                } else if parts.len() == 1 {
                    req.url.host = parts[0].to_string();
                    req.url.port = match req.url.scheme.as_str() {
                        "https" => 443,
                        _ => 80,
                    };
                }

                // Force scheme to http for local proxy redirects to prevent TLS handshakes
                req.url.scheme = "http".to_string();

                // Inject headers and resolve environment variables
                for (k, v) in &rule.inject_headers {
                    let resolved_value =
                        crate::variable::resolver::VariableResolver::resolve_env_vars(v);
                    if let (Ok(name), Ok(val)) = (
                        reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                        reqwest::header::HeaderValue::from_str(&resolved_value),
                    ) {
                        req.headers.insert(name, val);
                    }
                }
                break;
            }
        }
        Ok(())
    }

    async fn after_response(&self, _resp: &Response) -> Result<()> {
        Ok(())
    }
}
