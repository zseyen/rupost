use std::collections::HashMap;

use reqwest::{
    Body,
    header::{HeaderMap as Headers, HeaderName},
};

use crate::Result;
use crate::http::types::{Method, Url};
use serde::Serialize;

pub struct Request {
    pub method: Method,
    pub url: Url,
    pub headers: Headers,
    pub body: Option<Body>,
    pub query_params: HashMap<String, String>,
    /// 单请求总时长超时（覆盖 client 级 30s 默认）；流式请求（SSE/WS）不设置，
    /// 否则 reqwest 的总时长语义会在中途切断本应长连的流。
    pub timeout: Option<std::time::Duration>,
}

impl Request {
    pub fn new(method: &str, url: &str) -> Result<Self> {
        Ok(Self {
            method: method.parse()?,
            url: Url::parse(url)?,
            headers: Headers::new(),
            body: None,
            query_params: HashMap::new(),
            timeout: None,
        })
    }

    pub fn with_timeout(mut self, timeout: Option<std::time::Duration>) -> Self {
        self.timeout = timeout;
        self
    }

    fn insert_header(&mut self, key: &str, value: &str) -> Result<()> {
        let header_name: HeaderName = key.parse().map_err(|_| {
            crate::RupostError::RequestBuildFailed(format!("Invalid header name: '{}'", key))
        })?;
        let header_value = value.parse().map_err(|_| {
            crate::RupostError::RequestBuildFailed(format!("Invalid header value: '{}'", value))
        })?;
        self.headers.insert(header_name, header_value);
        Ok(())
    }

    pub fn with_header(mut self, key: &str, value: &str) -> Result<Self> {
        self.insert_header(key, value)?;
        Ok(self)
    }

    pub fn with_text(mut self, text: &str) -> Self {
        self.body = Some(Body::from(text.to_owned()));
        self
    }

    pub fn with_json<T: Serialize>(mut self, data: &T) -> Result<Self> {
        let json = serde_json::to_string(data)?;
        self.insert_header("Content-Type", "application/json")?;
        self.body = Some(Body::from(json));
        Ok(self)
    }
    pub fn with_body(mut self, body: &str) -> Self {
        self.body = Some(Body::from(body.to_owned()));
        self
    }

    pub fn with_query(mut self, key: &str, value: &str) -> Self {
        self.query_params.insert(key.to_string(), value.to_string());
        self
    }

    pub fn with_auth_bearer(mut self, token: &str) -> Result<Self> {
        self.insert_header("Authorization", &format!("Bearer {}", token))?;
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invalid_header_returns_error_not_panic() {
        // 回归：此前 header 名/值非法时用 expect() 直接 panic（如用户在 .http
        // 中手写 "X Bad Name: x"，或 {{var}} 渲染出垃圾）。应返回构建错误。
        let request = Request::new("GET", "http://example.com/").unwrap();

        let err = match request.with_header("X Bad Name", "x") {
            Err(e) => e,
            Ok(_) => panic!("expected invalid header name error"),
        };
        assert!(err.to_string().contains("Invalid header name"));

        let request2 = Request::new("GET", "http://example.com/").unwrap();
        let err2 = match request2.with_header("X-Fine", "bad\nvalue") {
            Err(e) => e,
            Ok(_) => panic!("expected invalid header value error"),
        };
        assert!(err2.to_string().contains("Invalid header value"));
    }

    #[test]
    fn test_header_valid_and_obstext_values_accepted() {
        let request = Request::new("GET", "http://example.com/")
            .unwrap()
            .with_header("X-Custom", "fine")
            .unwrap()
            // 合法 obs-text（0x80-0xFF 字节）是允许的 header 值
            .with_header("X-Note", "备注")
            .unwrap();
        assert_eq!(request.headers.get("X-Custom").unwrap(), "fine");
        assert_eq!(request.headers.get("X-Note").unwrap(), "备注");
    }
}
