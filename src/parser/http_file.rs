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

        // 跳过空行和注释
        while line_index < lines.len() {
            let line = lines[line_index].trim();
            if !line.is_empty() && !Self::is_comment(line) && !Self::is_metadata(line) {
                break;
            }
            line_index += 1;
            current_line += 1;
        }

        if line_index >= lines.len() {
            return Ok(None); // 空块
        }

        // 解析请求行（方法 + URL）
        let request_line = lines[line_index].trim();
        Self::parse_request_line(request_line, current_line, &mut request)?;
        line_index += 1;
        current_line += 1;

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
                current_line += 1;
                continue;
            }

            // 解析 header
            if let Some((key, value)) = Self::parse_header(line) {
                request.headers.push((key.to_string(), value.to_string()));
            } else {
                return Err(ParseError::InvalidHeader { line: current_line });
            }

            line_index += 1;
            current_line += 1;
        }

        // 解析 Body（空行后的所有内容）
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

        if parts.is_empty() {
            return Err(ParseError::MissingUrl { line: line_number });
        }

        // 可能的格式：
        // 1. URL (默认 GET)
        // 2. METHOD URL
        let valid_methods = ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"];

        if parts.len() == 1 {
            // 只有 URL，默认 GET
            request.url = parts[0].to_string();
        } else if parts.len() >= 2 {
            let first = parts[0].to_uppercase();
            if valid_methods.contains(&first.as_str()) {
                // METHOD URL
                request.method = Some(first);
                request.url = parts[1].to_string();
            } else {
                // 整个作为 URL（可能包含查询参数）
                request.url = parts.join(" ");
            }
        }

        Ok(())
    }

    /// 解析 header 行
    fn parse_header(line: &str) -> Option<(&str, &str)> {
        let colon_pos = line.find(':')?;
        let key = line[..colon_pos].trim();
        let value = line[colon_pos + 1..].trim();

        if key.is_empty() {
            return None;
        }

        Some((key, value))
    }

    /// 判断是否为注释行
    fn is_comment(line: &str) -> bool {
        line.starts_with('#') || line.starts_with("//")
    }

    /// 判断是否为元数据行（暂时只识别，不解析）
    fn is_metadata(line: &str) -> bool {
        line.starts_with('@')
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_get() {
        let content = "GET http://example.com";
        let result = HttpFileParser::parse_content(content).unwrap();

        assert_eq!(result.requests.len(), 1);
        let req = &result.requests[0];
        assert_eq!(req.method, Some("GET".to_string()));
        assert_eq!(req.url, "http://example.com");
        assert_eq!(req.headers.len(), 0);
        assert_eq!(req.body, None);
    }

    #[test]
    fn test_parse_url_only() {
        let content = "http://example.com";
        let result = HttpFileParser::parse_content(content).unwrap();

        assert_eq!(result.requests.len(), 1);
        let req = &result.requests[0];
        assert_eq!(req.method, None);
        assert_eq!(req.method_or_default(), "GET");
        assert_eq!(req.url, "http://example.com");
    }

    #[test]
    fn test_parse_with_headers() {
        let content = r#"
POST http://example.com/api
Content-Type: application/json
Authorization: Bearer token123
"#;
        let result = HttpFileParser::parse_content(content).unwrap();

        assert_eq!(result.requests.len(), 1);
        let req = &result.requests[0];
        assert_eq!(req.method, Some("POST".to_string()));
        assert_eq!(req.url, "http://example.com/api");
        assert_eq!(req.headers.len(), 2);
        assert_eq!(
            req.headers[0],
            ("Content-Type".to_string(), "application/json".to_string())
        );
        assert_eq!(
            req.headers[1],
            ("Authorization".to_string(), "Bearer token123".to_string())
        );
    }

    #[test]
    fn test_parse_with_body() {
        let content = r#"
POST http://example.com/api
Content-Type: application/json

{"name": "test", "value": 123}
"#;
        let result = HttpFileParser::parse_content(content).unwrap();

        assert_eq!(result.requests.len(), 1);
        let req = &result.requests[0];
        assert_eq!(req.method, Some("POST".to_string()));
        assert_eq!(
            req.body,
            Some(r#"{"name": "test", "value": 123}"#.to_string())
        );
    }

    #[test]
    fn test_parse_multiple_requests() {
        let content = r#"
GET http://example.com/users

###

POST http://example.com/users
Content-Type: application/json

{"name": "Alice"}
"#;
        let result = HttpFileParser::parse_content(content).unwrap();

        assert_eq!(result.requests.len(), 2);
        assert_eq!(result.requests[0].method, Some("GET".to_string()));
        assert_eq!(result.requests[0].url, "http://example.com/users");
        assert_eq!(result.requests[1].method, Some("POST".to_string()));
        assert_eq!(result.requests[1].url, "http://example.com/users");
    }

    #[test]
    fn test_parse_empty_content() {
        let content = "";
        let result = HttpFileParser::parse_content(content);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ParseError::NoRequests));
    }

    #[test]
    fn test_parse_with_comments() {
        let content = r#"
# This is a comment
GET http://example.com
// Another comment
User-Agent: RuPost
"#;
        let result = HttpFileParser::parse_content(content).unwrap();

        assert_eq!(result.requests.len(), 1);
        let req = &result.requests[0];
        assert_eq!(req.headers.len(), 1);
        assert_eq!(req.headers[0].0, "User-Agent");
    }
}
