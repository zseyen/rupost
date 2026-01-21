use crate::http::types::Status;
use anyhow::Result;
use reqwest::Body;
use reqwest::header::HeaderMap as Headers;
use std::time::Duration;

pub struct Response {
    pub status: Status,
    pub headers: Headers,
    pub body: Body,
    body_str: String,
    pub duration: Duration,
}

impl Response {
    pub fn new(status: u16, headers: Headers, body: String, duration: Duration) -> Result<Self> {
        Ok(Self {
            status: Status::new(status)?,
            headers,
            body: Body::from(body.clone()),
            body_str: body,
            duration,
        })
    }

    pub fn error(message: String) -> Self {
        Self {
            status: Status::new(500).unwrap(),
            headers: Headers::new(),
            body: Body::from(message.clone()),
            body_str: message,
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
        Ok(&self.body_str)
    }
}
