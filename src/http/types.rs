use std::str::FromStr;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
    Options,
}

impl FromStr for Method {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "GET" => Ok(Method::Get),
            "POST" => Ok(Method::Post),
            "PUT" => Ok(Method::Put),
            "DELETE" => Ok(Method::Delete),
            "PATCH" => Ok(Method::Patch),
            "HEAD" => Ok(Method::Head),
            "OPTIONS" => Ok(Method::Options),
            _ => bail!("Invalid HTTP method: {}", s),
        }
    }
}

impl Method {
    pub fn parse(s: &str) -> Result<Self> {
        s.parse()
    }

    pub fn as_str(&self) -> &str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Delete => "DELETE",
            Method::Patch => "PATCH",
            Method::Head => "HEAD",
            Method::Options => "OPTIONS",
        }
    }
}

pub struct Url {
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub path: String,
    pub query: String,
    pub fragment: String,
}

impl Url {
    /// 默认 host，当 URL 中未指定 host 时使用
    const DEFAULT_HOST: &'static str = "localhost";
    /// 默认 scheme，当 URL 中未指定 scheme 时使用
    const DEFAULT_SCHEME: &'static str = "http";

    pub fn parse(s: &str) -> Result<Self> {
        let input = s.trim();

        // 处理各种简化格式:
        // 1. ":3000" -> "http://localhost:3000"
        // 2. "localhost:3000" -> "http://localhost:3000"
        // 3. "https://:8080" -> "https://localhost:8080"
        let normalized = if input.starts_with(':') {
            // 纯端口号格式: ":3000"
            format!("{}://{}{}", Self::DEFAULT_SCHEME, Self::DEFAULT_HOST, input)
        } else if !input.contains("://") {
            // 无协议格式: "localhost:3000" 或 "example.com/path"
            format!("{}://{}", Self::DEFAULT_SCHEME, input)
        } else if let Some(pos) = input.find("://") {
            // 处理 "scheme://:port" 格式 (空 host)
            let after_scheme = &input[pos + 3..];
            if after_scheme.starts_with(':') {
                // "https://:8080" -> "https://localhost:8080"
                format!("{}://{}{}", &input[..pos], Self::DEFAULT_HOST, after_scheme)
            } else {
                input.to_string()
            }
        } else {
            input.to_string()
        };

        let url = url::Url::parse(&normalized)
            .map_err(|e| anyhow::anyhow!("Failed to parse URL '{}': {}", s, e))?;

        let default_port = match url.scheme() {
            "https" => 443,
            "http" => 80,
            _ => 80,
        };

        Ok(Url {
            scheme: url.scheme().to_string(),
            host: url
                .host()
                .map(|h| h.to_string())
                .unwrap_or_else(|| Self::DEFAULT_HOST.to_string()),
            port: url.port().unwrap_or(default_port),
            path: if url.path().is_empty() {
                "/".to_string()
            } else {
                url.path().to_string()
            },
            query: url.query().unwrap_or_default().to_string(),
            fragment: url.fragment().unwrap_or_default().to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_full_url() {
        let url = Url::parse("https://api.example.com:8443/v1/users?id=1#section").unwrap();
        assert_eq!(url.scheme, "https");
        assert_eq!(url.host, "api.example.com");
        assert_eq!(url.port, 8443);
        assert_eq!(url.path, "/v1/users");
        assert_eq!(url.query, "id=1");
        assert_eq!(url.fragment, "section");
    }

    #[test]
    fn test_parse_url_without_port_https() {
        let url = Url::parse("https://example.com/path").unwrap();
        assert_eq!(url.scheme, "https");
        assert_eq!(url.host, "example.com");
        assert_eq!(url.port, 443);
        assert_eq!(url.path, "/path");
    }

    #[test]
    fn test_parse_url_without_port_http() {
        let url = Url::parse("http://example.com/path").unwrap();
        assert_eq!(url.scheme, "http");
        assert_eq!(url.host, "example.com");
        assert_eq!(url.port, 80);
        assert_eq!(url.path, "/path");
    }

    #[test]
    fn test_parse_url_without_scheme() {
        let url = Url::parse("example.com/api/users").unwrap();
        assert_eq!(url.scheme, "http");
        assert_eq!(url.host, "example.com");
        assert_eq!(url.port, 80);
        assert_eq!(url.path, "/api/users");
    }

    #[test]
    fn test_parse_localhost_with_port() {
        let url = Url::parse("localhost:3000").unwrap();
        assert_eq!(url.scheme, "http");
        assert_eq!(url.host, "localhost");
        assert_eq!(url.port, 3000);
        assert_eq!(url.path, "/");
    }

    #[test]
    fn test_parse_port_only() {
        let url = Url::parse(":8080").unwrap();
        assert_eq!(url.scheme, "http");
        assert_eq!(url.host, "localhost");
        assert_eq!(url.port, 8080);
        assert_eq!(url.path, "/");
    }

    #[test]
    fn test_parse_port_with_scheme() {
        let url = Url::parse("https://:8080").unwrap();
        assert_eq!(url.scheme, "https");
        assert_eq!(url.host, "localhost");
        assert_eq!(url.port, 8080);
        assert_eq!(url.path, "/");
    }

    #[test]
    fn test_parse_port_with_path() {
        let url = Url::parse(":8080/path").unwrap();
        assert_eq!(url.scheme, "http");
        assert_eq!(url.host, "localhost");
        assert_eq!(url.port, 8080);
        assert_eq!(url.path, "/path");
    }

    #[test]
    fn test_parse_url_without_query_and_fragment() {
        let url = Url::parse("http://example.com/path").unwrap();
        assert_eq!(url.query, "");
        assert_eq!(url.fragment, "");
    }

    #[test]
    fn test_parse_url_with_whitespace() {
        let url = Url::parse("  http://example.com/path  ").unwrap();
        assert_eq!(url.host, "example.com");
        assert_eq!(url.path, "/path");
    }

    #[test]
    fn test_parse_localhost_with_path() {
        let url = Url::parse("localhost:3000/api/v1").unwrap();
        assert_eq!(url.scheme, "http");
        assert_eq!(url.host, "localhost");
        assert_eq!(url.port, 3000);
        assert_eq!(url.path, "/api/v1");
    }

    #[test]
    fn test_parse_ip_address() {
        let url = Url::parse("127.0.0.1:8080/test").unwrap();
        assert_eq!(url.scheme, "http");
        assert_eq!(url.host, "127.0.0.1");
        assert_eq!(url.port, 8080);
        assert_eq!(url.path, "/test");
    }
}
