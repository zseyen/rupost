//! RFC 7230 原始 HTTP 报文导入器 (Raw HTTP Importer)
//!
//! 解析标准的 HTTP/1.1 或 HTTP/2 请求报文，支持将 Host 头合并为完整 URL，
//! 并支持可选的 Host 变量模板化（`{{base_url}}`）。

use crate::Result;
use crate::error::RupostError;
use crate::http::url::{UrlTemplatizer, has_http_scheme};
use crate::importer::RequestImporter;
use crate::parser::types::ParsedRequest;

/// 原始 HTTP 报文导入器
pub struct RawHttpImporter;

impl RequestImporter for RawHttpImporter {
    fn parse(&self, content: &str, templatize_host: bool) -> Result<Vec<ParsedRequest>> {
        let req = Self::parse_single(content, templatize_host)?;
        Ok(vec![req])
    }
}

impl RawHttpImporter {
    /// 解析单条 Raw HTTP 报文
    pub fn parse_single(raw: &str, templatize_host: bool) -> Result<ParsedRequest> {
        let mut lines = raw.lines();
        let request_line = lines
            .next()
            .ok_or_else(|| RupostError::ParseError("Empty raw HTTP request".to_string()))?;

        let parts: Vec<&str> = request_line.split_whitespace().collect();
        if parts.len() < 2 {
            return Err(RupostError::ParseError(
                "Invalid HTTP request line (expected: METHOD URL [VERSION])".to_string(),
            ));
        }

        let method = parts[0].to_string();
        let mut url = parts[1].to_string();

        let mut headers = Vec::new();
        let mut body_lines = Vec::new();
        let mut in_body = false;

        for line in lines {
            if in_body {
                body_lines.push(line);
            } else if line.trim().is_empty() {
                in_body = true;
            } else if let Some((k, v)) = line.split_once(':') {
                let h_name = k.trim().to_string();
                let h_val = v.trim().to_string();
                if h_name.eq_ignore_ascii_case("host") && !has_http_scheme(&url) {
                    url = format!("http://{}{}", h_val, url);
                }
                headers.push((h_name, h_val));
            }
        }

        if templatize_host {
            url = UrlTemplatizer::templatize_origin(&url, "{{base_url}}");
        }

        let body = if body_lines.is_empty() {
            None
        } else {
            Some(body_lines.join("\n"))
        };

        Ok(ParsedRequest {
            method: Some(method),
            url,
            headers,
            body,
            metadata: Default::default(),
            line_number: 1,
            base_path: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_raw_http_basic() {
        let raw = "GET /api/v1/users HTTP/1.1\nHost: example.com\nAccept: application/json";
        let parsed = RawHttpImporter::parse_single(raw, false).unwrap();
        assert_eq!(parsed.method.as_deref(), Some("GET"));
        assert_eq!(parsed.url, "http://example.com/api/v1/users");
        assert_eq!(parsed.headers.len(), 2);
        assert!(parsed.body.is_none());
    }

    #[test]
    fn test_parse_raw_http_with_body_and_templatize() {
        let raw = "POST /api/v1/login HTTP/1.1\nHost: auth.example.com\nContent-Type: application/json\n\n{\"username\":\"admin\"}";
        let parsed = RawHttpImporter::parse_single(raw, true).unwrap();
        assert_eq!(parsed.method.as_deref(), Some("POST"));
        assert_eq!(parsed.url, "{{base_url}}/api/v1/login");
        assert_eq!(parsed.body.as_deref(), Some("{\"username\":\"admin\"}"));
    }

    #[test]
    fn test_parse_raw_http_empty_fails() {
        let res = RawHttpImporter::parse_single("", false);
        assert!(res.is_err());
    }
}
