use crate::Result;
use crate::error::RupostError;
use crate::http::url::UrlTemplatizer;
use crate::importer::curl::parse_curl_to_entry;
use crate::parser::types::ParsedRequest;
use crate::web::types::{ImportFormat, ImportRequest, ImportResponse};

/// 多格式装入器引擎
pub struct MultiFormatImporter;

impl MultiFormatImporter {
    /// 智能嗅探输入文本的格式
    pub fn sniff_format(content: &str) -> ImportFormat {
        let trimmed = content.trim();

        if trimmed.starts_with("curl ") || trimmed.starts_with("curl\n") {
            ImportFormat::Curl
        } else if trimmed.starts_with("http ")
            || trimmed.starts_with("https ")
            || trimmed.starts_with("http\n")
        {
            ImportFormat::Httpie
        } else if trimmed.starts_with('{')
            && trimmed.contains("\"log\"")
            && trimmed.contains("\"entries\"")
        {
            ImportFormat::Har
        } else if (trimmed.starts_with('{') || trimmed.starts_with("openapi:"))
            && (trimmed.contains("\"openapi\"")
                || trimmed.contains("openapi:")
                || trimmed.contains("swagger:"))
        {
            ImportFormat::OpenApi
        } else if trimmed.starts_with('{') && trimmed.contains("\"_postman_id\"") {
            ImportFormat::Postman
        } else if Self::is_raw_http(trimmed) {
            ImportFormat::RawHttp
        } else {
            // 默认回退为 Curl 尝试
            ImportFormat::Curl
        }
    }

    /// 检测是否是 Raw HTTP 1.1 / RFC 7230 报文
    fn is_raw_http(text: &str) -> bool {
        let first_line = text.lines().next().unwrap_or("").trim();
        let methods = ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"];
        for m in &methods {
            if first_line.starts_with(m)
                && (first_line.contains("HTTP/1.") || first_line.contains("HTTP/2"))
            {
                return true;
            }
        }
        false
    }

    /// 执行统一装入转换
    pub fn import(req: &ImportRequest) -> Result<ImportResponse> {
        let format = if req.format == ImportFormat::Auto {
            Self::sniff_format(&req.content)
        } else {
            req.format
        };

        let mut warnings = Vec::new();

        let parsed_requests = match format {
            ImportFormat::Curl | ImportFormat::Auto => {
                let entry = parse_curl_to_entry(&req.content)?;
                let url = if req.templatize_host {
                    UrlTemplatizer::templatize_origin(&entry.request.url, "{{base_url}}")
                } else {
                    entry.request.url
                };
                let mut headers = Vec::new();
                for (k, v) in &entry.request.headers {
                    headers.push((k.as_str().to_string(), v.to_str().unwrap_or("").to_string()));
                }
                let parsed = ParsedRequest {
                    method: Some(entry.request.method),
                    url,
                    headers,
                    body: entry.request.body,
                    metadata: Default::default(),
                    line_number: 1,
                    base_path: None,
                };
                vec![parsed]
            }
            ImportFormat::RawHttp => {
                let parsed = Self::parse_raw_http(&req.content, req.templatize_host)?;
                vec![parsed]
            }
            ImportFormat::Httpie => {
                let parsed = Self::parse_httpie(&req.content, req.templatize_host)?;
                vec![parsed]
            }
            other => {
                warnings.push(format!(
                    "格式 {:?} 暂按 Raw 模式尝试解析并生成代码块",
                    other
                ));
                let parsed = Self::parse_raw_http(&req.content, req.templatize_host)?;
                vec![parsed]
            }
        };

        let mut markdown_blocks = Vec::new();
        for r in &parsed_requests {
            let block = Self::format_as_markdown_block(r, req.generate_mock);
            markdown_blocks.push(block);
        }

        Ok(ImportResponse {
            detected_format: format,
            total_imported: markdown_blocks.len(),
            markdown_blocks,
            warnings,
        })
    }

    /// 解析 Raw HTTP 报文
    pub fn parse_raw_http(raw: &str, templatize_host: bool) -> Result<ParsedRequest> {
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
                if h_name.eq_ignore_ascii_case("host") && !url.starts_with("http") {
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

    /// 解析 HTTPie 终端命令 (e.g. `http POST :8000/api/users name=alice`)
    pub fn parse_httpie(cmd: &str, templatize_host: bool) -> Result<ParsedRequest> {
        let trimmed = cmd.trim();
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        if tokens.is_empty() {
            return Err(RupostError::ParseError("Empty HTTPie command".to_string()));
        }

        // 跳过命令名 (http / https)
        let mut idx = 1;
        let mut method = "GET".to_string();
        let methods = ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"];

        if idx < tokens.len() && methods.contains(&tokens[idx].to_uppercase().as_str()) {
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

    /// 将解析出的请求转换为 Markdown 活文档代码块
    pub fn format_as_markdown_block(req: &ParsedRequest, generate_mock: bool) -> String {
        let mut buf = String::with_capacity(512);
        buf.push_str("```http\n");

        let method = req.method.as_deref().unwrap_or("GET");
        buf.push_str(&format!("{} {}\n", method, req.url));

        for (k, v) in &req.headers {
            buf.push_str(&format!("{}: {}\n", k, v));
        }

        if let Some(ref body) = req.body {
            buf.push('\n');
            buf.push_str(body.trim());
            buf.push('\n');
        }

        if generate_mock {
            buf.push_str("\n@mock-default\nHTTP/1.1 200 OK\nContent-Type: application/json\n\n{\n  \"status\": \"success\",\n  \"message\": \"Mock response generated by Rupost\"\n}\n");
        }

        buf.push_str("```");
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sniff_format_capabilities() {
        assert_eq!(
            MultiFormatImporter::sniff_format("curl -X POST https://example.com"),
            ImportFormat::Curl
        );
        assert_eq!(
            MultiFormatImporter::sniff_format("http POST :8000/api/users name=bob"),
            ImportFormat::Httpie
        );
        assert_eq!(
            MultiFormatImporter::sniff_format("GET /api/v1/ping HTTP/1.1\nHost: localhost"),
            ImportFormat::RawHttp
        );
        assert_eq!(
            MultiFormatImporter::sniff_format(
                "{\n  \"log\": {\n    \"version\": \"1.2\",\n    \"entries\": []\n  }\n}"
            ),
            ImportFormat::Har
        );
    }

    #[test]
    fn test_import_curl_generates_markdown_block() {
        let req = ImportRequest {
            format: ImportFormat::Auto,
            content: "curl -X POST https://api.example.com/users -H 'Content-Type: application/json' -d '{\"name\":\"alice\"}'".to_string(),
            templatize_host: true,
            generate_mock: true,
        };

        let res = MultiFormatImporter::import(&req).unwrap();
        assert_eq!(res.detected_format, ImportFormat::Curl);
        assert_eq!(res.total_imported, 1);
        let block = &res.markdown_blocks[0];
        assert!(block.contains("POST {{base_url}}/users"));
        assert!(block.contains("@mock-default"));
        assert!(block.contains("HTTP/1.1 200 OK"));
    }

    #[test]
    fn test_import_httpie_generates_markdown_block() {
        let req = ImportRequest {
            format: ImportFormat::Httpie,
            content: "http POST :9000/api/login user=admin pass=123456".to_string(),
            templatize_host: false,
            generate_mock: false,
        };

        let res = MultiFormatImporter::import(&req).unwrap();
        assert_eq!(res.detected_format, ImportFormat::Httpie);
        let block = &res.markdown_blocks[0];
        assert!(block.contains("POST http://localhost:9000/api/login"));
        assert!(block.contains("\"user\": \"admin\""));
        assert!(!block.contains("@mock-default"));
    }

    #[test]
    fn test_import_raw_http_generates_markdown_block() {
        let raw = "POST /api/v1/orders HTTP/1.1\nHost: example.com\nContent-Type: application/json\n\n{\"order_id\": \"1001\"}";
        let req = ImportRequest {
            format: ImportFormat::Auto,
            content: raw.to_string(),
            templatize_host: true,
            generate_mock: true,
        };

        let res = MultiFormatImporter::import(&req).unwrap();
        assert_eq!(res.detected_format, ImportFormat::RawHttp);
        let block = &res.markdown_blocks[0];
        assert!(block.contains("POST {{base_url}}/api/v1/orders"));
        assert!(block.contains("@mock-default"));
    }

    #[test]
    fn test_import_idn_chinese_domain_safety() {
        // 中文国际化域名导入（杜绝切片越界 Panic）
        let raw = "POST http://测试.com:8080/api/v1/orders HTTP/1.1\nHost: 测试.com:8080\n\n{\"order_id\": \"1001\"}";
        let req = ImportRequest {
            format: ImportFormat::Auto,
            content: raw.to_string(),
            templatize_host: true,
            generate_mock: false,
        };

        let res = MultiFormatImporter::import(&req).unwrap();
        assert_eq!(res.detected_format, ImportFormat::RawHttp);
        let block = &res.markdown_blocks[0];
        assert!(block.contains("POST {{base_url}}/api/v1/orders"));
    }

    #[test]
    fn test_import_httpie_query_params_and_templatize() {
        let req = ImportRequest {
            format: ImportFormat::Httpie,
            content: "http GET https://api.example.com/items category==books limit==10".to_string(),
            templatize_host: true,
            generate_mock: false,
        };

        let res = MultiFormatImporter::import(&req).unwrap();
        assert_eq!(res.detected_format, ImportFormat::Httpie);
        let block = &res.markdown_blocks[0];
        assert!(block.contains("GET {{base_url}}/items?category=books&limit=10"));
    }
}
