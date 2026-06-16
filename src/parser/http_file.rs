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

        // 提取依赖
        file.dependencies = Self::extract_dependencies(content);

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

    /// 提取全局依赖
    fn extract_dependencies(content: &str) -> Vec<String> {
        let mut deps = Vec::new();
        for line in content.lines() {
            let trimmed = line.trim();
            let has_depends = (trimmed.starts_with("###") || trimmed.starts_with('#'))
                && trimmed.contains("@depends-on");
            if let Some(pos) = if has_depends {
                trimmed.find("@depends-on")
            } else {
                None
            } {
                let dep = trimmed[pos + "@depends-on".len()..].trim();
                let dep = dep.trim_end_matches("-->").trim();
                if !dep.is_empty() {
                    deps.push(dep.to_string());
                }
            }
        }
        deps
    }

    /// 判定是否是合法的请求行 (如 "GET http://example.com")
    fn is_valid_request_line(line: &str) -> bool {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            return false;
        }
        let method = parts[0].to_uppercase();
        let valid_methods = ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"];
        valid_methods.contains(&method.as_str()) && parts.len() >= 2
    }

    /// 启发式自适应请求块拆分
    fn split_by_separator(content: &str) -> Vec<(String, usize)> {
        let mut blocks = Vec::new();
        let mut current_block = String::new();
        let mut block_start_line = 1;

        // 用于状态感知型拆分的局部状态变量
        let mut has_req_line = false;
        let mut last_req_method: Option<String> = None;
        let mut has_empty_line_after_req_line = false;

        for (current_line, line) in (1..).zip(content.lines()) {
            let trimmed = line.trim();

            let is_explicit_separator = trimmed.starts_with("###");

            // 识别新用例的元数据开始指令 (例如 @name 或 @test)
            let is_new_case_metadata = trimmed.starts_with("@name")
                || trimmed.starts_with("@test");

            let is_req_line = Self::is_valid_request_line(trimmed);

            // 判定是否应当切分
            let mut should_split = is_explicit_separator;

            if !should_split && has_req_line {
                // 如果当前块中已经包含过请求行了：
                // 1. 遇到了新用例的元数据开始指令 (比如 @name 或 @test)，说明一定是下一个请求的元数据
                // 2. 符合请求行特征：
                //    - 如果在请求行后尚未遇到过空行（Headers 区域连写），切分
                //    - 如果已遇到空行，但上一个请求是无 Body 的方法 (GET, DELETE, HEAD, OPTIONS)，说明空行后不能 be Body 文本，必须切分
                if is_new_case_metadata {
                    should_split = true;
                } else if is_req_line {
                    if !has_empty_line_after_req_line {
                        should_split = true;
                    } else if let Some(ref method) = last_req_method {
                        let no_body_methods = ["GET", "DELETE", "HEAD", "OPTIONS"];
                        if no_body_methods.contains(&method.as_str()) {
                            should_split = true;
                        }
                    }
                }
            }

            if should_split && !current_block.trim().is_empty() {
                blocks.push((current_block.clone(), block_start_line));
                current_block.clear();
                block_start_line = current_line;
                has_req_line = false;
                last_req_method = None;
                has_empty_line_after_req_line = false;
            }

            if is_req_line {
                has_req_line = true;
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if !parts.is_empty() {
                    last_req_method = Some(parts[0].to_uppercase());
                }
            }

            // 在请求行之后遇到空行，更新状态
            if has_req_line && trimmed.is_empty() {
                has_empty_line_after_req_line = true;
            }

            // 显式分隔符行本身不需要被加入到请求文本中
            if !trimmed.starts_with("###") {
                current_block.push_str(line);
                current_block.push('\n');
            }
        }

        if !current_block.trim().is_empty() {
            blocks.push((current_block, block_start_line));
        }

        blocks
    }

    /// 解析单个请求块
    fn parse_request_block(block: &str, start_line: usize) -> ParseResult<Option<ParsedRequest>> {
        let mut request = ParsedRequest::new(start_line);
        let mut http_lines = Vec::new();
        let mut current_line = start_line;

        for line in block.lines() {
            let trimmed = line.trim();
            let metadata = if trimmed.starts_with('@') {
                metadata::parse_metadata(trimmed)?
            } else {
                None
            };

            if let Some(metadata) = metadata {
                metadata::apply_metadata(&metadata, &mut request.metadata);
                current_line += 1;
                continue; // 成功提取元数据，该行不作为 HTTP 报文行
            }
            http_lines.push((line, current_line));
            current_line += 1;
        }

        let mut line_index = 0;

        // 跳过前导空行和注释
        while line_index < http_lines.len() {
            let (line, _) = http_lines[line_index];
            let trimmed = line.trim();
            if trimmed.is_empty() || Self::is_comment(trimmed) {
                line_index += 1;
                continue;
            }
            break;
        }

        if line_index >= http_lines.len() {
            return Ok(None); // 没有请求内容
        }

        // 解析请求行（方法 + URL）
        let (request_line, req_line_num) = http_lines[line_index];
        Self::parse_request_line(request_line.trim(), req_line_num, &mut request)?;
        line_index += 1;

        // 解析 Headers
        while line_index < http_lines.len() {
            let (line, _) = http_lines[line_index];
            let trimmed = line.trim();

            // 空行表示 headers 结束，body 开始
            if trimmed.is_empty() {
                line_index += 1;
                break;
            }

            // 跳过注释
            if Self::is_comment(trimmed) {
                line_index += 1;
                continue;
            }

            // 解析 header
            if let Some((key, value)) = Self::parse_header(trimmed) {
                request.headers.push((key.to_string(), value.to_string()));
            }

            line_index += 1;
        }

        // 解析 Body（空行后所有内容）
        if line_index < http_lines.len() {
            let body_parts: Vec<&str> = http_lines[line_index..].iter().map(|(l, _)| *l).collect();
            let body = body_parts.join("\n");
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
    pub(crate) fn parse_request_line(
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
    pub(crate) fn parse_header(line: &str) -> Option<(&str, &str)> {
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
    fn test_parse_metadata_at_the_end() {
        let content = r#"
GET http://example.com
Content-Type: application/json

{"name": "test"}
@assert status == 200
@assert body.success == true
        "#;
        let result = HttpFileParser::parse_content(content).unwrap();
        assert_eq!(result.requests.len(), 1);
        let req = &result.requests[0];
        assert_eq!(req.metadata.assertions.len(), 2);
        assert_eq!(req.metadata.assertions[0], "status == 200");
        assert_eq!(req.metadata.assertions[1], "body.success == true");
        assert_eq!(req.body.as_deref(), Some(r#"{"name": "test"}"#));
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

    #[test]
    fn test_parse_multiple_requests_without_separator() {
        let content = r#"
@name test-1
GET http://example.com/1

@name test-2
POST http://example.com/2

{"id": 1}
        "#;
        let result = HttpFileParser::parse_content(content).unwrap();
        assert_eq!(result.requests.len(), 2);

        assert_eq!(result.requests[0].metadata.name, Some("test-1".to_string()));
        assert_eq!(result.requests[0].method, Some("GET".to_string()));
        assert_eq!(result.requests[0].url, "http://example.com/1");
        assert_eq!(result.requests[0].body, None);

        assert_eq!(result.requests[1].metadata.name, Some("test-2".to_string()));
        assert_eq!(result.requests[1].method, Some("POST".to_string()));
        assert_eq!(result.requests[1].url, "http://example.com/2");
        assert_eq!(
            result.requests[1].body,
            Some("{\"id\": 1}".to_string())
        );
    }

    #[test]
    fn test_parse_no_separator_mixed_with_separator() {
        let content = r#"
@name test-1
GET http://example.com/1

###

@name test-2
POST http://example.com/2

@name test-3
GET http://example.com/3
        "#;
        let result = HttpFileParser::parse_content(content).unwrap();
        assert_eq!(result.requests.len(), 3);
        assert_eq!(result.requests[0].url, "http://example.com/1");
        assert_eq!(result.requests[1].url, "http://example.com/2");
        assert_eq!(result.requests[2].url, "http://example.com/3");
    }

    #[test]
    fn test_parse_body_containing_http_verb() {
        // 验证 Body 内即便顶格写了以 HTTP 动作开头的文本，也不会被错误截断或误切
        let content = r#"
POST http://example.com/api
Content-Type: text/plain

GET http://someurl.com/should/not/be/split
This is still body.
        "#;
        let result = HttpFileParser::parse_content(content).unwrap();
        assert_eq!(result.requests.len(), 1);
        assert_eq!(
            result.requests[0].body.as_deref(),
            Some("GET http://someurl.com/should/not/be/split\nThis is still body.")
        );
    }
}
