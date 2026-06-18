use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteSegment {
    Literal(String),
    Param(String), // id in :id
    Wildcard,      // *
    MultiWildcard, // **
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
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        self.insert_segments(&segments, data);
    }

    fn insert_segments(&mut self, segments: &[&str], data: T) {
        if segments.is_empty() {
            self.data = Some(data);
            return;
        }

        let first = segments[0];
        let segment_type = RouteSegment::parse(first);

        let child_idx =
            if let Some(idx) = self.children.iter().position(|c| c.segment == segment_type) {
                idx
            } else {
                self.children.push(TrieNode::new(segment_type));
                self.children.len() - 1
            };

        self.children[child_idx].insert_segments(&segments[1..], data);
    }

    pub fn match_path(&self, path: &str) -> Option<(&T, HashMap<String, String>)> {
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut params = HashMap::new();
        if let Some(data) = self.match_segments(&segments, &mut params) {
            Some((data, params))
        } else {
            None
        }
    }

    fn match_segments(
        &self,
        segments: &[&str],
        params: &mut HashMap<String, String>,
    ) -> Option<&T> {
        if segments.is_empty() {
            return self.data.as_ref();
        }

        let first = segments[0];

        // 1. Literal 精确匹配
        for child in &self.children {
            if let RouteSegment::Literal(ref lit) = child.segment
                && lit == first
                && let Some(res) = child.match_segments(&segments[1..], params)
            {
                return Some(res);
            }
        }

        // 2. Param 路径变量匹配 (如 :id)
        for child in &self.children {
            if let RouteSegment::Param(ref param_name) = child.segment {
                let prev_val = params.insert(param_name.clone(), first.to_string());
                if let Some(res) = child.match_segments(&segments[1..], params) {
                    return Some(res);
                }
                // 回溯
                if let Some(v) = prev_val {
                    params.insert(param_name.clone(), v);
                } else {
                    params.remove(param_name);
                }
            }
        }

        // 3. Wildcard 单段通配符匹配 (如 *)
        for child in &self.children {
            if matches!(child.segment, RouteSegment::Wildcard)
                && let Some(res) = child.match_segments(&segments[1..], params)
            {
                return Some(res);
            }
        }

        // 4. MultiWildcard 多段通配符匹配 (如 **)
        for child in &self.children {
            if matches!(child.segment, RouteSegment::MultiWildcard)
                && let Some(ref data) = child.data
            {
                return Some(data);
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trie_literal_match() {
        let mut root = TrieNode::new(RouteSegment::Wildcard); // Root segment represents wildcard or dummy
        root.insert("/api/v1/users", "users_list");
        root.insert("/api/v2/orders", "orders_list");

        let res1 = root.match_path("/api/v1/users");
        assert!(res1.is_some());
        let (data, params) = res1.unwrap();
        assert_eq!(*data, "users_list");
        assert!(params.is_empty());

        let res2 = root.match_path("/api/v1/orders");
        assert!(res2.is_none());
    }

    #[test]
    fn test_trie_param_capture() {
        let mut root = TrieNode::new(RouteSegment::Wildcard);
        root.insert("/api/users/:id", "user_detail");
        root.insert("/api/users/:id/posts/:post_id", "post_detail");

        let res1 = root.match_path("/api/users/123");
        assert!(res1.is_some());
        let (data, params) = res1.unwrap();
        assert_eq!(*data, "user_detail");
        assert_eq!(params.get("id").unwrap(), "123");

        let res2 = root.match_path("/api/users/456/posts/789");
        assert!(res2.is_some());
        let (data2, params2) = res2.unwrap();
        assert_eq!(*data2, "post_detail");
        assert_eq!(params2.get("id").unwrap(), "456");
        assert_eq!(params2.get("post_id").unwrap(), "789");
    }

    #[test]
    fn test_trie_wildcard_match() {
        let mut root = TrieNode::new(RouteSegment::Wildcard);
        root.insert("/api/*/config", "config_data");

        let res1 = root.match_path("/api/users/config");
        assert!(res1.is_some());
        assert_eq!(*res1.unwrap().0, "config_data");

        let res2 = root.match_path("/api/orders/config");
        assert!(res2.is_some());

        let res3 = root.match_path("/api/users/profile/config");
        assert!(res3.is_none());
    }

    #[test]
    fn test_trie_multi_wildcard_match() {
        let mut root = TrieNode::new(RouteSegment::Wildcard);
        root.insert("/static/**", "static_file");

        let res1 = root.match_path("/static/js/main.js");
        assert!(res1.is_some());
        assert_eq!(*res1.unwrap().0, "static_file");

        let res2 = root.match_path("/static/css/theme/dark.css");
        assert!(res2.is_some());

        let res3 = root.match_path("/static/logo.png");
        assert!(res3.is_some());

        let res4 = root.match_path("/api/static/logo.png");
        assert!(res4.is_none());
    }
}
