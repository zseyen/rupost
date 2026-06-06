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
        let _ = method;
        let _ = path;
        let _ = variants;
        todo!()
    }
}

impl MockMatcher for TrieRouteMatcher {
    fn match_request(&self, req: &MockRequest) -> Option<MockResponse> {
        let _ = req;
        todo!()
    }
}

impl Default for TrieRouteMatcher {
    fn default() -> Self {
        Self::new()
    }
}
