use crate::parser::http_file::HttpFileParser;
use crate::parser::types::{
    FileMetadata, ParseResult, ParsedFile, ParsedMockVariant, ParsedRequest,
};
use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};
use std::path::Path;

#[derive(Debug, PartialEq, Eq)]
enum ParseState {
    RequestLineAndHeaders,
    VariantHeaders,
    VariantBody,
}

/// Markdown 文件解析器
pub struct MarkdownFileParser;

impl MarkdownFileParser {
    /// 从文件路径解析
    pub fn parse_file<P: AsRef<Path>>(path: P) -> ParseResult<ParsedFile> {
        let content = std::fs::read_to_string(&path)?;
        let mut parsed = Self::parse_content(&content)?;
        parsed.source_path = Some(path.as_ref().to_path_buf());
        Ok(parsed)
    }

    /// 从字符串内容解析
    pub fn parse_content(content: &str) -> ParseResult<ParsedFile> {
        let mut parsed_file = ParsedFile::new();

        // 提取 YAML Frontmatter
        let trimmed_content = content.trim_start();
        if let Some(stripped) = trimmed_content.strip_prefix("---") {
            let find_end = stripped.find("---");
            if let Some(end_pos) = find_end {
                let yaml_str = &stripped[..end_pos];
                if let Ok(meta) = serde_yaml::from_str::<FileMetadata>(yaml_str) {
                    parsed_file.metadata = meta;
                }
            }
        }

        let code_blocks = Self::extract_code_blocks(content);

        // 提取依赖
        parsed_file.dependencies = Self::extract_dependencies(content);

        for block in code_blocks {
            let has_mock_variants =
                block.content.contains("@mock-when") || block.content.contains("@mock-default");

            if has_mock_variants {
                let mut req = Self::parse_mock_block(&block.content, 1)?;
                if req.metadata.name.is_none() {
                    req.metadata.name = block.preceding_header.clone();
                }
                parsed_file.add_request(req);
            } else {
                // 解析代码块内容为请求
                let mut block_parsed = HttpFileParser::parse_content(&block.content)?;

                // 为每个请求设置名称（如果没有明确 of @name）及测试标识
                let is_test = block.content.contains("@test");
                for req in &mut block_parsed.requests {
                    if req.metadata.name.is_none() {
                        req.metadata.name = block.preceding_header.clone();
                    }
                    req.metadata.is_test = is_test;
                }

                parsed_file.requests.extend(block_parsed.requests);
            }
        }

        Ok(parsed_file)
    }

    fn parse_mock_block(block_content: &str, start_line: usize) -> ParseResult<ParsedRequest> {
        let mut request = ParsedRequest::new(start_line);
        let mut state = ParseState::RequestLineAndHeaders;

        let mut current_variant: Option<ParsedMockVariant> = None;
        let mut current_body_lines = Vec::new();

        for (line_idx, line) in block_content.lines().enumerate() {
            let current_line_num = start_line + line_idx;
            let trimmed = line.trim();

            match state {
                ParseState::RequestLineAndHeaders => {
                    if trimmed.starts_with("@mock-when") || trimmed.starts_with("@mock-default") {
                        Self::process_variant_line(
                            trimmed,
                            &mut current_variant,
                            &mut request,
                            &mut current_body_lines,
                        );
                        state = ParseState::VariantHeaders;
                    } else if trimmed.starts_with('@') {
                        if let Some(meta) = crate::parser::metadata::parse_metadata(trimmed)? {
                            crate::parser::metadata::apply_metadata(&meta, &mut request.metadata);
                        }
                    } else if request.url.is_empty() {
                        if !trimmed.is_empty()
                            && !trimmed.starts_with('#')
                            && !trimmed.starts_with("//")
                        {
                            HttpFileParser::parse_request_line(
                                trimmed,
                                current_line_num,
                                &mut request,
                            )?;
                        }
                    } else if !trimmed.is_empty()
                        && !trimmed.starts_with('#')
                        && !trimmed.starts_with("//")
                    {
                        let header = HttpFileParser::parse_header(trimmed);
                        if let Some((k, v)) = header {
                            request.headers.push((k.to_string(), v.to_string()));
                        }
                    }
                }
                ParseState::VariantHeaders => {
                    if trimmed.is_empty() {
                        state = ParseState::VariantBody;
                    } else if trimmed.starts_with("HTTP/1.1") || trimmed.starts_with("HTTP/2") {
                        let parts: Vec<&str> = trimmed.split_whitespace().collect();
                        if parts.len() >= 2 {
                            let parsed_code = parts[1].parse::<u16>();
                            if let Ok(code) = parsed_code {
                                let var_opt = current_variant.as_mut();
                                if let Some(var) = var_opt {
                                    var.status = code;
                                }
                            }
                        }
                    } else if let Some((k, v)) = HttpFileParser::parse_header(trimmed) {
                        let var_opt = current_variant.as_mut();
                        if let Some(var) = var_opt {
                            var.headers.push((k.to_string(), v.to_string()));
                        }
                    }
                }
                ParseState::VariantBody => {
                    if trimmed.starts_with("@mock-when") || trimmed.starts_with("@mock-default") {
                        Self::process_variant_line(
                            trimmed,
                            &mut current_variant,
                            &mut request,
                            &mut current_body_lines,
                        );
                        state = ParseState::VariantHeaders;
                    } else {
                        current_body_lines.push(line);
                    }
                }
            }
        }

        if let Some(mut var) = current_variant {
            var.body = Some(current_body_lines.join("\n"));
            request.metadata.mock_variants.push(var);
        }

        request.metadata.is_test = block_content.contains("@test");

        Ok(request)
    }

    fn process_variant_line(
        trimmed: &str,
        current_variant: &mut Option<ParsedMockVariant>,
        request: &mut ParsedRequest,
        current_body_lines: &mut Vec<&str>,
    ) {
        if let Some(mut var) = current_variant.take() {
            var.body = Some(current_body_lines.join("\n"));
            request.metadata.mock_variants.push(var);
            current_body_lines.clear();
        }

        if let Some(stripped) = trimmed.strip_prefix("@mock-when") {
            let cond = stripped.trim().to_string();
            *current_variant = Some(ParsedMockVariant {
                condition_expr: Some(cond),
                status: 200,
                headers: Vec::new(),
                body: None,
            });
        } else if trimmed.starts_with("@mock-default") {
            *current_variant = Some(ParsedMockVariant {
                condition_expr: None,
                status: 200,
                headers: Vec::new(),
                body: None,
            });
        }
    }

    /// 从 Markdown 文件内容中解析并提取该文件声明的全局拓扑依赖文件。
    ///
    /// # 依赖声明格式支持
    /// 该函数会逐行扫描，识别以 `#`、`###` 或 `<!--` 开头，且包含 `@depends-on` 关键字的行。
    /// 具体支持以下几种常见的依赖声明格式：
    /// - Markdown 标题/注释格式：`# @depends-on other_file.http`
    /// - HTTP 分隔符/指令格式：`### @depends-on other_file.md`
    /// - HTML 注释嵌入格式：`<!-- @depends-on other_file.http -->` （尾部的 `-->` 会被自动剥离）
    ///
    /// # 参数
    /// * `content` - Markdown 文件的文本内容字符串
    ///
    /// # 返回值
    /// 返回包含所有提取到的依赖文件路径/名称的字符串向量 `Vec<String>`。
    fn extract_dependencies(content: &str) -> Vec<String> {
        let mut deps = Vec::new();
        for line in content.lines() {
            let trimmed = line.trim();
            let has_depends = (trimmed.starts_with("###")
                || trimmed.starts_with('#')
                || trimmed.starts_with("<!--"))
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

    /// 提取所有 http/rest 代码块（使用 pulldown-cmark）
    fn extract_code_blocks(content: &str) -> Vec<ExtractedCodeBlock> {
        let parser = Parser::new(content);

        let mut blocks = Vec::new();
        let mut current_header: Option<String> = None;
        let mut in_code_block = false;
        let mut current_code = String::new();
        let mut is_capturing_header = false;
        let mut header_text = String::new();

        for event in parser {
            match event {
                // 标题开始
                Event::Start(Tag::Heading { .. }) => {
                    is_capturing_header = true;
                    header_text.clear();
                }

                // 标题结束
                Event::End(TagEnd::Heading(..)) => {
                    if is_capturing_header && !header_text.is_empty() {
                        current_header = Some(header_text.clone());
                    }
                    is_capturing_header = false;
                }

                // 文本内容
                Event::Text(text) => {
                    if is_capturing_header {
                        // 收集标题文本
                        header_text.push_str(&text);
                    } else if in_code_block {
                        // 收集代码块内容
                        current_code.push_str(&text);
                    }
                }

                // 代码块开始
                Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(lang))) => {
                    let lang_str = lang.to_string().to_lowercase();
                    if lang_str == "http" || lang_str == "rest" {
                        in_code_block = true;
                        current_code.clear();
                    }
                }

                // 代码块结束
                Event::End(TagEnd::CodeBlock) if in_code_block => {
                    blocks.push(ExtractedCodeBlock {
                        content: current_code.clone(),
                        preceding_header: current_header.clone(),
                    });

                    in_code_block = false;
                    current_code.clear();
                }

                _ => {}
            }
        }

        blocks
    }
}

#[derive(Debug)]
struct ExtractedCodeBlock {
    content: String,
    preceding_header: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_single_http_block() {
        let content = r#"
# API Docs

## Get Users

```http
GET https://api.example.com/users
```
"#;
        let parsed = MarkdownFileParser::parse_content(content).unwrap();
        assert_eq!(parsed.requests.len(), 1);
        assert_eq!(
            parsed.requests[0].metadata.name,
            Some("Get Users".to_string())
        );
    }

    #[test]
    fn test_extract_multiple_blocks() {
        let content = r#"
# User API

## List Users

```http
GET https://api.example.com/users
```

## Create User

```rest
POST https://api.example.com/users
Content-Type: application/json

{"name": "Alice"}
```
"#;
        let parsed = MarkdownFileParser::parse_content(content).unwrap();
        assert_eq!(parsed.requests.len(), 2);
        assert_eq!(
            parsed.requests[0].metadata.name,
            Some("List Users".to_string())
        );
        assert_eq!(
            parsed.requests[1].metadata.name,
            Some("Create User".to_string())
        );
    }

    #[test]
    fn test_explicit_name_overrides_header() {
        let content = r#"
## Get Users

```http
@name custom-name
GET https://api.example.com/users
```
"#;
        let parsed = MarkdownFileParser::parse_content(content).unwrap();
        assert_eq!(
            parsed.requests[0].metadata.name,
            Some("custom-name".to_string())
        );
    }

    #[test]
    fn test_empty_file() {
        let content = "";
        let parsed = MarkdownFileParser::parse_content(content).unwrap();
        assert_eq!(parsed.requests.len(), 0);
    }

    #[test]
    fn test_no_code_blocks() {
        let content = r#"
# API Documentation

This is just text without any code blocks.

## Overview

More text here.
"#;
        let parsed = MarkdownFileParser::parse_content(content).unwrap();
        assert_eq!(parsed.requests.len(), 0);
    }

    #[test]
    fn test_metadata_in_code_block() {
        let content = r#"
## Metadata Test

```http
@name WithMetadata
@assert status == 200
@skip
GET https://api.example.com
```
"#;
        let parsed = MarkdownFileParser::parse_content(content).unwrap();
        assert_eq!(parsed.requests.len(), 1);
        let req = &parsed.requests[0];
        assert_eq!(req.metadata.name, Some("WithMetadata".to_string()));
        assert_eq!(req.metadata.assertions.len(), 1);
        assert_eq!(req.metadata.assertions[0], "status == 200");
        assert!(req.metadata.skip);
    }

    #[test]
    fn test_parse_frontmatter() {
        let content = r#"---
title: User API Specification
version: 2.1.0
base_path: /api/v2
rules: |
  1. No sensitive plain passwords.
security:
  - type: BearerAuth
---

# User API
"#;
        let parsed = MarkdownFileParser::parse_content(content).unwrap();
        let meta = parsed.metadata;
        assert_eq!(meta.title.as_deref(), Some("User API Specification"));
        assert_eq!(meta.version.as_deref(), Some("2.1.0"));
        assert_eq!(meta.base_path.as_deref(), Some("/api/v2"));
        assert_eq!(
            meta.rules.as_deref(),
            Some("1. No sensitive plain passwords.\n")
        );
    }

    #[test]
    fn test_parse_mock_variants() {
        let content = r#"
## Get User Profile

```http
GET /api/v1/users/:id

@mock-when query.role == admin
HTTP/1.1 200 OK
Content-Type: application/json

{
  "id": "{{id}}",
  "name": "Admin"
}

@mock-default
HTTP/1.1 200 OK
Content-Type: application/json

{
  "id": "{{id}}",
  "name": "Default"
}
```
"#;
        let parsed = MarkdownFileParser::parse_content(content).unwrap();
        assert_eq!(parsed.requests.len(), 1);
        let req = &parsed.requests[0];
        assert!(!req.metadata.is_test);
        assert_eq!(req.metadata.mock_variants.len(), 2);

        // 验证变体一
        let var1 = &req.metadata.mock_variants[0];
        assert_eq!(var1.condition_expr.as_deref(), Some("query.role == admin"));
        assert_eq!(var1.status, 200);
        assert!(
            var1.headers
                .iter()
                .any(|(k, v)| k == "Content-Type" && v == "application/json")
        );
        assert!(var1.body.as_ref().unwrap().contains("Admin"));

        // 验证变体二
        let var2 = &req.metadata.mock_variants[1];
        assert!(var2.condition_expr.is_none());
        assert_eq!(var2.status, 200);
        assert!(var2.body.as_ref().unwrap().contains("Default"));
    }

    #[test]
    fn test_distinguish_test_block() {
        let content = r#"
## Test admin profile flow

```http
@name test-admin
@test
GET http://localhost:9000/api/v1/users/123?role=admin
@assert status == 200
```
"#;
        let parsed = MarkdownFileParser::parse_content(content).unwrap();
        assert_eq!(parsed.requests.len(), 1);
        let req = &parsed.requests[0];
        assert!(req.metadata.is_test);
    }
}
