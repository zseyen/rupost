use std::collections::HashMap;
use crate::mock::trie::TrieNode;
use crate::mock::variant::MockVariant;

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
}

pub trait MockMatcher: Send + Sync {
    fn match_request(&self, req: &MockRequest) -> Option<MockResponse>;
}

pub struct TrieRouteMatcher {
    // 每一个 Method (如 GET, POST) 都有自己的 Trie 树
    pub routes: HashMap<String, TrieNode<Vec<MockVariant>>>,
}

impl TrieRouteMatcher {
    pub fn new() -> Self {
        Self {
            routes: HashMap::new(),
        }
    }

    pub fn add_route(&mut self, method: &str, path: &str, variants: Vec<MockVariant>) {
        let method_upper = method.to_uppercase();
        let trie = self
            .routes
            .entry(method_upper)
            .or_insert_with(|| TrieNode::new(crate::mock::trie::RouteSegment::Wildcard));
        trie.insert(path, variants);
    }
}

impl MockMatcher for TrieRouteMatcher {
    fn match_request(&self, req: &MockRequest) -> Option<MockResponse> {
        let method_upper = req.method.to_uppercase();
        let trie = self.routes.get(&method_upper)?;
        let (variants, params) = trie.match_path(&req.path)?;

        for variant in variants {
            let matched = if let Some(ref cond) = variant.condition {
                cond.evaluate(&req.headers, &req.query, &req.body)
            } else {
                true
            };

            if matched {
                // Populate path parameters into variable context
                let mut context = crate::variable::VariableContext::new();
                for (k, v) in params {
                    context.insert(k, v);
                }

                // Subsitute placeholder variables in response body & headers
                use crate::variable::VariableResolver;
                let resolved_body = VariableResolver::resolve(&variant.response_body, &context);

                let mut resolved_headers = HashMap::new();
                for (k, v) in &variant.headers {
                    resolved_headers.insert(k.clone(), VariableResolver::resolve(v, &context));
                }

                return Some(MockResponse {
                    status: variant.status,
                    headers: resolved_headers,
                    body: resolved_body,
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
