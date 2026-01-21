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
}

impl Request {
    pub fn new(method: &str, url: &str) -> Result<Self> {
        Ok(Self {
            method: method.parse()?,
            url: Url::parse(url)?,
            headers: Headers::new(),
            body: None,
            query_params: HashMap::new(),
        })
    }

    fn insert_header(&mut self, key: &str, value: &str) {
        let header_name: HeaderName = key.parse().expect("invalid header name");
        self.headers
            .insert(header_name, value.parse().expect("invalid header value"));
    }

    pub fn with_header(mut self, key: &str, value: &str) -> Self {
        self.insert_header(key, value);
        self
    }

    pub fn with_text(mut self, text: &str) -> Self {
        self.body = Some(Body::from(text.to_owned()));
        self
    }

    pub fn with_json<T: Serialize>(mut self, data: &T) -> Result<Self> {
        let json = serde_json::to_string(data)?;
        self.insert_header("Content-Type", "application/json");
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

    pub fn with_auth_bearer(mut self, token: &str) -> Self {
        self.insert_header("Authorization", &format!("Bearer {}", token));
        self
    }
}
