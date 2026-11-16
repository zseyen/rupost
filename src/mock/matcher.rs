use crate::mock::trie::{RouteSegment, TrieNode};
use crate::mock::variant::{EvalContext, MockVariant};
use crate::variable::{VariableContext, VariableResolver};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone)]
pub struct MockRequest {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub query: HashMap<String, String>,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct MockResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
    pub matched_pattern: Option<String>,
}

pub trait MockMatcher: Send + Sync {
    fn match_request(&self, req: &MockRequest) -> Option<MockResponse>;
}

/// 支持运行时原子替换内部 Matcher 的线程安全包装器（用于 Web 原地热重载）
pub struct SwappableMatcher {
    inner: RwLock<Arc<dyn MockMatcher>>,
}

impl SwappableMatcher {
    pub fn new(initial: Arc<dyn MockMatcher>) -> Self {
        Self {
            inner: RwLock::new(initial),
        }
    }

    pub fn swap(&self, next: Arc<dyn MockMatcher>) {
        let mut guard = self.inner.write().unwrap_or_else(|e| e.into_inner());
        *guard = next;
    }
}

impl MockMatcher for SwappableMatcher {
    fn match_request(&self, req: &MockRequest) -> Option<MockResponse> {
        let guard = self.inner.read().unwrap_or_else(|e| e.into_inner());
        guard.match_request(req)
    }
}

pub struct TrieRouteMatcher {
    // 每一个 Method (如 GET, POST) 都有自己的 Trie 树
    pub routes: HashMap<String, TrieNode<(String, Vec<MockVariant>)>>,
}

impl TrieRouteMatcher {
    pub fn new() -> Self {
        Self {
            routes: HashMap::new(),
        }
    }

    pub fn add_route(&mut self, method: &str, path: &str, mut variants: Vec<MockVariant>) {
        let method_upper = method.to_uppercase();
        let trie = self
            .routes
            .entry(method_upper)
            .or_insert_with(|| TrieNode::new(RouteSegment::Wildcard));

        variants.sort_by_key(|v| v.condition.is_none());

        trie.upsert(
            path,
            (path.to_string(), variants),
            |(_, existing_vars), (_, new_vars)| {
                existing_vars.extend(new_vars);
                existing_vars.sort_by_key(|v| v.condition.is_none());
            },
        );
    }

    /// 只读匹配路由并捕获路径参数（供 Web 仿真探针复用，避免双套匹配逻辑）
    pub fn match_route_with_params(
        &self,
        method: &str,
        path: &str,
    ) -> Option<(&str, &[MockVariant], HashMap<String, String>)> {
        let method_upper = method.to_uppercase();
        let trie = self.routes.get(&method_upper)?;
        let ((pattern, variants), params) = trie.match_path(path)?;
        Some((pattern.as_str(), variants.as_slice(), params))
    }
}

impl MockMatcher for TrieRouteMatcher {
    fn match_request(&self, req: &MockRequest) -> Option<MockResponse> {
        let (pattern, variants, params) = self.match_route_with_params(&req.method, &req.path)?;
        let pattern = pattern.to_string();
        let eval_ctx = EvalContext::new(&req.headers, &req.query, &req.body);

        for variant in variants {
            let matched = if let Some(ref cond) = variant.condition {
                cond.evaluate_in(&eval_ctx)
            } else {
                true
            };

            if matched {
                // Populate path parameters into variable context
                let mut context = VariableContext::new();
                for (k, v) in params {
                    context.insert(k, v);
                }

                // Substitute placeholder variables in response body & headers
                let resolved_body = VariableResolver::resolve(&variant.response_body, &context);

                let mut resolved_headers = HashMap::new();
                for (k, v) in &variant.headers {
                    resolved_headers.insert(k.clone(), VariableResolver::resolve(v, &context));
                }

                return Some(MockResponse {
                    status: variant.status,
                    headers: resolved_headers,
                    body: resolved_body,
                    matched_pattern: Some(pattern),
                });
            }
        }
        None
    }
}

impl Default for TrieRouteMatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MockRouteConfig {
    pub method: String,
    pub path: String,
    pub variants: Vec<MockVariant>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::variant::{CompareOp, ConditionSource, VariantCondition};

    #[test]
    fn test_add_route_merges_variants_and_keeps_default_last() {
        let mut matcher = TrieRouteMatcher::new();
        matcher.add_route(
            "GET",
            "/api/items/:id",
            vec![MockVariant {
                condition: None,
                status: 404,
                headers: HashMap::new(),
                response_body: "default".to_string(),
            }],
        );
        matcher.add_route(
            "GET",
            "/api/items/{id}",
            vec![MockVariant {
                condition: Some(VariantCondition {
                    source: ConditionSource::Query,
                    key: "role".to_string(),
                    operator: CompareOp::Equals,
                    expected_value: "admin".to_string(),
                }),
                status: 200,
                headers: HashMap::new(),
                response_body: "admin:{{id}}".to_string(),
            }],
        );

        let mut query = HashMap::new();
        query.insert("role".to_string(), "admin".to_string());
        let resp = matcher
            .match_request(&MockRequest {
                method: "GET".to_string(),
                path: "/api/items/7".to_string(),
                headers: HashMap::new(),
                query,
                body: String::new(),
            })
            .expect("should match merged route");
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, "admin:7");
    }
}
