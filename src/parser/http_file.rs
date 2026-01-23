use crate::parser::metadata;
use crate::parser::types::{ParseError, ParseResult, ParsedFile, ParsedRequest};
use std::path::Path;

/// HTTP 文件解析器
pub struct HttpFileParser;

impl HttpFileParser {
    /// 从文件路径解析
    pub fn parse_file<P: AsRef<Path>>(path: P) -> ParseResult<ParsedFile> {
        let content = std::fs::read_to_string(path.as_ref())?;
        let mut parsed = Self::parse_content(&content)?;
        parsed.source_path = Some(path.as_ref().to_path_buf());
        Ok(parsed)
    }

    /// 从字符串内容解析
    pub fn parse_content(content: &str) -> ParseResult<ParsedFile> {
        let mut file = ParsedFile::new();

        // 按 ### 分割请求块
        let blocks = Self::split_by_separator(content);

        if blocks.is_empty() {
            return Err(ParseError::NoRequests);
        }

        for (block, start_line) in blocks {
            if let Some(request) = Self::parse_request_block(&block, start_line)? {
                file.add_request(request);
            }
        }

        if file.requests.is_empty() {
            return Err(ParseError::NoRequests);
        }

        Ok(file)
    }

    /// 按 ### 分隔符分割内容
    fn split_by_separator(content: &str) -> Vec<(String, usize)> {
        let mut blocks = Vec::new();
        let mut current_block = String::new();
        let mut block_start_line = 1;
        let mut current_line = 1;

        for line in content.lines() {
            if line.trim().starts_with("###") {
                // 遇到分隔符，保存当前块
                if !current_block.trim().is_empty() {
                    blocks.push((current_block.clone(), block_start_line));
                }
                current_block.clear();
                block_start_line = current_line + 1;
            } else {
                current_block.push_str(line);
                current_block.push('\n');
            }
            current_line += 1;
        }

        // 添加最后一个块
        if !current_block.trim().is_empty() {
            blocks.push((current_block, block_start_line));
        }

        // 如果没有找到分隔符，整个内容作为一个块
        if blocks.is_empty() && !content.trim().is_empty() {
            blocks.push((content.to_string(), 1));
        }

        blocks
    }

    /// 解析单个请求块
    fn parse_request_block(block: &str, start_line: usize) -> ParseResult<Option<ParsedRequest>> {
        let lines: Vec<&str> = block.lines().collect();

        if lines.is_empty() {
            return Ok(None);
        }

        let mut request = ParsedRequest::new(start_line);
        let mut line_index = 0;
        let mut current_line = start_line;

        // 解析元数据和跳过空行/注释
        while line_index < lines.len() {
            let line = lines[line_index].trim();

            if line.is_empty() || Self::is_comment(line) {
                line_index += 1;
                current_line += 1;
                continue;
            }

            // 解析元数据
            if line.starts_with('@') {
                if let Some(metadata) = metadata::parse_metadata(line)? {
                    metadata::apply_metadata(&metadata, &mut request.metadata);
                }
                line_index += 1;
                current_line += 1;
                continue;
            }

            // 遇到非元数据行，结束元数据解析
            break;
        }

        if line_index >= lines.len() {
            return Ok(None); // 只有元数据，没有请求
        }

        // 解析请求行（方法 + URL）
        let request_line = lines[line_index].trim();
        Self::parse_request_line(request_line, current_line, &mut request)?;
        line_index += 1;

        // 解析 Headers
        while line_index < lines.len() {
            let line = lines[line_index].trim();

            // 空行表示 headers 结束，body 开始
            if line.is_empty() {
                line_index += 1;
                break;
            }

            // 跳过注释
            if Self::is_comment(line) {
                line_index += 1;
                continue;
            }

            // 解析 header
            if let Some((key, value)) = Self::parse_header(line) {
                request.headers.push((key.to_string(), value.to_string()));
            }

            line_index += 1;
        }

        // 解析 Body（空行后只内容）
        if line_index < lines.len() {
            let body = lines[line_index..].join("\n");
            let body = body.trim();
            if !body.is_empty() {
                request.body = Some(body.to_string());
            }
        }

        // 验证 URL
        if request.url.is_empty() {
            return Err(ParseError::MissingUrl { line: start_line });
        }

        Ok(Some(request))
    }

    /// 解析请求行（方法 + URL）
    fn parse_request_line(
        line: &str,
        line_number: usize,
        request: &mut ParsedRequest,
    ) -> ParseResult<()> {
        let parts: Vec<&str> = line.split_whitespace().collect();

        match parts.len() {
            0 => {
                return Err(ParseError::InvalidFormat {
                    line: line_number,
                    message: "Empty request line".to_string(),
                });
            }
            1 => {
                // 只有 URL，方法默认为 GET
                request.url = parts[0].to_string();
                request.method = None;
            }
            2 => {
                // 方法 + URL
                let method = parts[0].to_uppercase();
                let valid_methods = ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"];
                if !valid_methods.contains(&method.as_str()) {
                    return Err(ParseError::InvalidMethod {
                        method,
                        line: line_number,
                    });
                }
                request.method = Some(method);
                request.url = parts[1].to_string();
            }
            _ => {
                return Err(ParseError::InvalidFormat {
                    line: line_number,
                    message: "Too many parts in request line".to_string(),
                });
            }
        }

        Ok(())
    }

    /// 解析 header 行
    fn parse_header(line: &str) -> Option<(&str, &str)> {
        if let Some(colon_pos) = line.find(':') {
            let key = line[..colon_pos].trim();
            let value = line[colon_pos + 1..].trim();
            if !key.is_empty() {
                return Some((key, value));
            }
        }
        None
    }

    /// 判断是否为注释行
    fn is_comment(line: &str) -> bool {
        line.starts_with('#') || line.starts_with("//")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::metadata;
    use std::time::Duration;

    #[test]
    fn test_parse_simple_get() {
        let content = "GET http://example.com";
        let result = HttpFileParser::parse_content(content).unwrap();
        assert_eq!(result.requests.len(), 1);
        assert_eq!(result.requests[0].method, Some("GET".to_string()));
        assert_eq!(result.requests[0].url, "http://example.com");
    }

    #[test]
    fn test_parse_url_only() {
        let content = "http://example.com";
        let result = HttpFileParser::parse_content(content).unwrap();
        assert_eq!(result.requests.len(), 1);
        assert_eq!(result.requests[0].method, None);
        assert_eq!(result.requests[0].url, "http://example.com");
    }

    #[test]
    fn test_parse_with_headers() {
        let content = r#"
POST http://example.com
Content-Type: application/json
Authorization: Bearer token123
        "#;
        let result = HttpFileParser::parse_content(content).unwrap();
        assert_eq!(result.requests.len(), 1);
        assert_eq!(result.requests[0].method, Some("POST".to_string()));
        assert_eq!(result.requests[0].headers.len(), 2);
        assert_eq!(result.requests[0].headers[0].0, "Content-Type");
        assert_eq!(result.requests[0].headers[0].1, "application/json");
        assert_eq!(result.requests[0].headers[1].0, "Authorization");
        assert_eq!(result.requests[0].headers[1].1, "Bearer token123");
    }

    #[test]
    fn test_parse_with_body() {
        let content = r#"
POST http://example.com
Content-Type: application/json

{"name": "test"}
        "#;
        let result = HttpFileParser::parse_content(content).unwrap();
        assert_eq!(result.requests.len(), 1);
        assert_eq!(
            result.requests[0].body,
            Some(r#"{"name": "test"}"#.to_string())
        );
    }

    #[test]
    fn test_parse_multiple_requests() {
        let content = r#"
GET http://example.com/1

###

POST http://example.com/2
        "#;
        let result = HttpFileParser::parse_content(content).unwrap();
        assert_eq!(result.requests.len(), 2);
        assert_eq!(result.requests[0].url, "http://example.com/1");
        assert_eq!(result.requests[1].url, "http://example.com/2");
    }

    #[test]
    fn test_parse_empty_content() {
        let content = "";
        let result = HttpFileParser::parse_content(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_with_comments() {
        let content = r#"
# This is a comment
GET http://example.com
// Another comment
        "#;
        let result = HttpFileParser::parse_content(content).unwrap();
        assert_eq!(result.requests.len(), 1);
    }

    #[test]
    fn test_parse_name_metadata() {
        let content = "@name My Test\nGET http://example.com";
        let result = HttpFileParser::parse_content(content).unwrap();
        assert_eq!(
            result.requests[0].metadata.name,
            Some("My Test".to_string())
        );
    }

    #[test]
    fn test_parse_skip_metadata() {
        let content = "@skip\nGET http://example.com";
        let result = HttpFileParser::parse_content(content).unwrap();
        assert!(result.requests[0].metadata.skip);
    }

    #[test]
    fn test_parse_timeout_metadata() {
        let content = "@timeout 5s\nGET http://example.com";
        let result = HttpFileParser::parse_content(content).unwrap();
        assert_eq!(
            result.requests[0].metadata.timeout,
            Some(Duration::from_secs(5))
        );
    }

    #[test]
    fn test_parse_duration_formats() {
        assert_eq!(
            metadata::parse_duration("1000ms").unwrap(),
            Duration::from_millis(1000)
        );
        assert_eq!(
            metadata::parse_duration("5s").unwrap(),
            Duration::from_secs(5)
        );
        assert_eq!(
            metadata::parse_duration("2m").unwrap(),
            Duration::from_secs(120)
        );
    }

    #[test]
    fn test_parse_assert_metadata() {
        let content = "@assert status == 200\n@assert body.id > 0\nGET http://example.com";
        let result = HttpFileParser::parse_content(content).unwrap();
        assert_eq!(result.requests[0].metadata.assertions.len(), 2);
        assert_eq!(result.requests[0].metadata.assertions[0], "status == 200");
        assert_eq!(result.requests[0].metadata.assertions[1], "body.id > 0");
    }

    #[test]
    fn test_parse_multiple_metadata() {
        let content = r#"
@name Test Request
@timeout 5s
@assert status == 200
@assert body.token exists
POST http://example.com
"#;
        let result = HttpFileParser::parse_content(content).unwrap();
        assert_eq!(
            result.requests[0].metadata.name,
            Some("Test Request".to_string())
        );
        assert_eq!(
            result.requests[0].metadata.timeout,
            Some(Duration::from_secs(5))
        );
        assert_eq!(result.requests[0].metadata.assertions.len(), 2);
    }
}
