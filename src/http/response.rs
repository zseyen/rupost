use crate::http::types::Status;
use anyhow::Result;
use reqwest::header::HeaderMap as Headers;
use std::time::Duration;

pub struct Response {
    pub status: Status,
    pub headers: Headers,
    pub body: String, // 直接使用 String，不需要 reqwest::Body
    pub duration: Duration,
}

impl Response {
    pub fn new(status: u16, headers: Headers, body: String, duration: Duration) -> Result<Self> {
        Ok(Self {
            status: Status::new(status)?,
            headers,
            body, // 直接使用，无需 clone
            duration,
        })
    }

    pub fn error(message: String) -> Self {
        Self {
            status: Status::new(500).unwrap(),
            headers: Headers::new(),
            body: message, // 直接使用，无需 clone
            duration: Duration::from_millis(0),
        }
    }

    pub fn is_success(&self) -> bool {
        self.status.is_success()
    }

    pub fn is_redirect(&self) -> bool {
        self.status.is_redirect()
    }

    pub fn is_client_error(&self) -> bool {
        self.status.is_client_error()
    }

    pub fn is_server_error(&self) -> bool {
        self.status.is_server_error()
    }

    pub fn text(&self) -> Result<&str> {
        Ok(&self.body)
    }
}
