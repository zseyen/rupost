//! 通用 HTTP 请求导入领域模块 (Universal Request Importer Domain Module)
//!
//! 依据 Clean Architecture 原则，将原本混杂在 Web 适配层中的各类格式解析与转换逻辑下沉为
//! 独立的通用领域能力。零 Axum / Web 依赖，既可供 Web 工作台 API 路由复用，亦可供 CLI
//! （如 `rupost import`）直接使用。

pub mod curl;
pub mod httpie;
pub mod raw;

use crate::Result;
use crate::parser::types::ParsedRequest;
pub use curl::{CurlImportRequest, CurlImporter, run_curl_import};
pub use httpie::HttpieImporter;
pub use raw::RawHttpImporter;
use serde::{Deserialize, Serialize};

/// 通用请求导入器契约 Trait
pub trait RequestImporter: Send + Sync {
    /// 将输入文本内容解析为一组标准抽象请求 `ParsedRequest`
    fn parse(&self, content: &str, templatize_host: bool) -> Result<Vec<ParsedRequest>>;
}

/// 支持的外部请求导入格式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportFormat {
    Auto,
    Curl,
    Httpie,
    RawHttp,
    Har,
    OpenApi,
    Postman,
}

pub fn default_import_format() -> ImportFormat {
    ImportFormat::Auto
}

/// 统一导入请求入参
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportRequest {
    #[serde(default = "default_import_format")]
    pub format: ImportFormat,
    pub content: String,
    #[serde(default)]
    pub templatize_host: bool,
    #[serde(default)]
    pub generate_mock: bool,
}

/// 统一导入响应出参
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResponse {
    pub detected_format: ImportFormat,
    pub markdown_blocks: Vec<String>,
    pub total_imported: usize,
    pub warnings: Vec<String>,
}

/// 多格式统一装入器引擎
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
    pub fn is_raw_http(text: &str) -> bool {
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
                CurlImporter.parse(&req.content, req.templatize_host)?
            }
            ImportFormat::RawHttp => RawHttpImporter.parse(&req.content, req.templatize_host)?,
            ImportFormat::Httpie => HttpieImporter.parse(&req.content, req.templatize_host)?,
            other => {
                warnings.push(format!(
                    "格式 {:?} 暂按 Raw 模式尝试解析并生成代码块",
                    other
                ));
                RawHttpImporter.parse(&req.content, req.templatize_host)?
            }
        };

        let mut markdown_blocks = Vec::with_capacity(parsed_requests.len());
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

    /// 便捷代理：解析 Raw HTTP
    pub fn parse_raw_http(raw: &str, templatize_host: bool) -> Result<ParsedRequest> {
        RawHttpImporter::parse_single(raw, templatize_host)
    }

    /// 便捷代理：解析 HTTPie
    pub fn parse_httpie(cmd: &str, templatize_host: bool) -> Result<ParsedRequest> {
        HttpieImporter::parse_single(cmd, templatize_host)
    }

    /// 便捷代理：解析 cURL
    pub fn parse_curl(cmd: &str, templatize_host: bool) -> Result<ParsedRequest> {
        let mut reqs = CurlImporter.parse(cmd, templatize_host)?;
        reqs.pop().ok_or_else(|| {
            crate::error::RupostError::ParseError("Empty parsed cURL request".to_string())
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
        assert_eq!(
            MultiFormatImporter::sniff_format("{\n  \"_postman_id\": \"123\"\n}"),
            ImportFormat::Postman
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
