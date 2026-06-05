use crate::parser::http_file::HttpFileParser;
use crate::parser::types::{ParseResult, ParsedFile};
use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};
use std::path::Path;

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
        let code_blocks = Self::extract_code_blocks(content);

        let mut parsed_file = ParsedFile::new();

        // 提取依赖
        parsed_file.dependencies = Self::extract_dependencies(content);

        for block in code_blocks {
            // 解析代码块内容为请求
            let mut block_parsed = HttpFileParser::parse_content(&block.content)?;

            // 为每个请求设置名称（如果没有明确 of @name）
            for req in &mut block_parsed.requests {
                if req.metadata.name.is_none() {
                    req.metadata.name = block.preceding_header.clone();
                }
            }

            parsed_file.requests.extend(block_parsed.requests);
        }

        Ok(parsed_file)
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
}
