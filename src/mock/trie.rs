use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteSegment {
    Literal(String),
    Param(String),      // id in :id
    Wildcard,           // *
    MultiWildcard,      // **
}

impl RouteSegment {
    pub fn parse(s: &str) -> Self {
        if s == "*" {
            RouteSegment::Wildcard
        } else if s == "**" {
            RouteSegment::MultiWildcard
        } else if s.starts_with(':') && s.len() > 1 {
            RouteSegment::Param(s[1..].to_string())
        } else {
            RouteSegment::Literal(s.to_string())
        }
    }
}

pub struct TrieNode<T> {
    pub segment: RouteSegment,
    pub children: Vec<TrieNode<T>>,
    pub data: Option<T>,
}

impl<T> TrieNode<T> {
    pub fn new(segment: RouteSegment) -> Self {
        Self {
            segment,
            children: Vec::new(),
            data: None,
        }
    }

    pub fn insert(&mut self, path: &str, data: T) {
        let _ = path;
        let _ = data;
        todo!()
    }

    pub fn match_path(&self, path: &str) -> Option<(&T, HashMap<String, String>)> {
        let _ = path;
        todo!()
    }
}
