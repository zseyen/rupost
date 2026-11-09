//! HTTPie 命令行语法导入器 (HTTPie Importer)
//!
//! 解析 `http` / `https` 命令行指令，支持：
//! - 简写 URL（`:8000/api` → `http://localhost:8000/api`）
//! - 自定义标头（`Header:Value`）
//! - 查询参数（`param==val`）
//! - JSON 请求体字段（`field=val`）
//! - Host 模板化（`{{base_url}}`）

use crate::Result;
use crate::error::RupostError;
use crate::http::Method;
use crate::http::url::UrlTemplatizer;
use crate::importer::RequestImporter;
use crate::parser::types::ParsedRequest;

/// HTTPie 命令行导入器
pub struct HttpieImporter;

impl RequestImporter for HttpieImporter {
    fn parse(&self, content: &str, templatize_host: bool) -> Result<Vec<ParsedRequest>> {
        let req = Self::parse_single(content, templatize_host)?;
        Ok(vec![req])
    }
}

impl HttpieImporter {
    /// 解析 HTTPie 终端命令 (e.g. `http POST :8000/api/users name=alice`)
    pub fn parse_single(cmd: &str, templatize_host: bool) -> Result<ParsedRequest> {
        let trimmed = cmd.trim();
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        if tokens.is_empty() {
            return Err(RupostError::ParseError("Empty HTTPie command".to_string()));
        }

        // 跳过命令名 (http / https)
        let mut idx = 1;
        let mut method = "GET".to_string();

        if idx < tokens.len() && Method::is_standard(tokens[idx]) {
            method = tokens[idx].to_uppercase();
            idx += 1;
        }

        let raw_url = if idx < tokens.len() {
            tokens[idx]
        } else {
            return Err(RupostError::ParseError(
                "Missing URL in HTTPie command".to_string(),
            ));
        };
        idx += 1;

        // HTTPie 支持简写 URL (e.g. :8000/items -> http://localhost:8000/items)
        let mut url = if raw_url.starts_with(':') {
            format!("http://localhost{}", raw_url)
        } else if !raw_url.starts_with("http://") && !raw_url.starts_with("https://") {
            format!("http://{}", raw_url)
        } else {
            raw_url.to_string()
        };

        let mut headers = Vec::new();
        let mut json_fields = Vec::new();
        let mut query_params = Vec::new();

        while idx < tokens.len() {
            let token = tokens[idx];
            if let Some((k, v)) = token.split_once(':') {
                headers.push((k.to_string(), v.to_string()));
            } else if let Some((k, v)) = token.split_once("==") {
                query_params.push((k.to_string(), v.to_string()));
            } else if let Some((k, v)) = token.split_once('=') {
                json_fields.push(format!("\"{}\": \"{}\"", k, v));
            }
            idx += 1;
        }

        // 拼接 query params 到 url
        if !query_params.is_empty() {
            let sep = if url.contains('?') { "&" } else { "?" };
            let query_str = query_params
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join("&");
            url = format!("{}{}{}", url, sep, query_str);
        }

        if templatize_host {
            url = UrlTemplatizer::templatize_origin(&url, "{{base_url}}");
        }

        let body = if !json_fields.is_empty() {
            if method == "GET" {
                method = "POST".to_string();
            }
            headers.push(("Content-Type".to_string(), "application/json".to_string()));
            Some(format!("{{\n  {}\n}}", json_fields.join(",\n  ")))
        } else {
            None
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
    fn test_parse_httpie_basic_get() {
        let cmd = "http GET :3000/health";
        let parsed = HttpieImporter::parse_single(cmd, false).unwrap();
        assert_eq!(parsed.method.as_deref(), Some("GET"));
        assert_eq!(parsed.url, "http://localhost:3000/health");
    }

    #[test]
    fn test_parse_httpie_post_json() {
        let cmd = "http POST https://api.com/users name=alice role=admin";
        let parsed = HttpieImporter::parse_single(cmd, false).unwrap();
        assert_eq!(parsed.method.as_deref(), Some("POST"));
        assert_eq!(parsed.url, "https://api.com/users");
        assert!(
            parsed
                .body
                .as_ref()
                .unwrap()
                .contains("\"name\": \"alice\"")
        );
        assert!(
            parsed
                .body
                .as_ref()
                .unwrap()
                .contains("\"role\": \"admin\"")
        );
    }

    #[test]
    fn test_parse_httpie_query_params() {
        let cmd = "http GET https://api.com/items filter==active page==1";
        let parsed = HttpieImporter::parse_single(cmd, true).unwrap();
        assert_eq!(parsed.url, "{{base_url}}/items?filter=active&page=1");
    }
}
