use crate::parser::http_file::HttpFileParser;
use crate::parser::types::{FileMetadata, ParseResult, ParsedFile, ParsedMockVariant};
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
        let mut parsed_file = ParsedFile::new();

        // 提取 Frontmatter (支持 +++ 与 --- 定界符，优先原生 TOML，兼容器双模兼容旧 key: value)
        let trimmed_content = content.trim_start();
        let frontmatter_str = if let Some(stripped) = trimmed_content.strip_prefix("+++") {
            stripped.find("+++").map(|end_pos| &stripped[..end_pos])
        } else if let Some(stripped) = trimmed_content.strip_prefix("---") {
            stripped.find("---").map(|end_pos| &stripped[..end_pos])
        } else {
            None
        };

        if let Some(fm) = frontmatter_str {
            parsed_file.metadata = Self::parse_frontmatter(fm);
        }

        let code_blocks = Self::extract_code_blocks(content);

        // 提取依赖
        parsed_file.dependencies = Self::extract_dependencies(content);

        let base_path = parsed_file.metadata.base_path.clone();

        for block in code_blocks {
            let split_result = Self::split_mock_variants(&block.content);

            if let Some((request_part, variants_part)) = split_result {
                // WP-P2a：block.line_number 是围栏开行，块内容首行 = 围栏 + 1
                let parsed_opt =
                    HttpFileParser::parse_request_block(&request_part, block.line_number + 1)?;
                if let Some(mut req) = parsed_opt {
                    if req.metadata.name.is_none() {
                        req.metadata.name = block.preceding_header.clone();
                    }
                    if req.base_path.is_none() {
                        req.base_path = base_path.clone();
                    }
                    let is_test = block.content.contains("@test");
                    req.metadata.is_test = is_test;

                    let variants = Self::parse_mock_variants_part(&variants_part);
                    req.metadata.mock_variants = variants;

                    parsed_file.add_request(req);
                }
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
                    if req.base_path.is_none() {
                        req.base_path = base_path.clone();
                    }
                    // WP-P2a：parse_content 内部从 1 计数，换算回文档行号
                    // （内容首行 = 围栏 + 1 = line_number + 1，relative 1 对应 +1）
                    req.metadata.source_line =
                        req.metadata.source_line.map(|n| n + block.line_number);
                }

                parsed_file.requests.extend(block_parsed.requests);
            }
        }

        Ok(parsed_file)
    }

    /// 解析 Frontmatter 文本：优先尝试标准 TOML，若失败则优雅兼容旧键值对语法
    fn parse_frontmatter(fm: &str) -> FileMetadata {
        // 1. 优先尝试原生 TOML 结构化反序列化
        if let Ok(meta) = toml::from_str::<FileMetadata>(fm) {
            return meta;
        }

        // 2. 优雅降级：轻量行扫描解析 key: value 及 rules: | 多行语法（无需依赖 YAML）
        let mut meta = FileMetadata::default();
        let mut current_multiline_key: Option<String> = None;
        let mut multiline_buf = String::new();

        for line in fm.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // 处理缩进的多行文本块
            if let Some(ref key) = current_multiline_key {
                if line.starts_with("  ") || line.starts_with('\t') {
                    multiline_buf.push_str(trimmed);
                    multiline_buf.push('\n');
                    continue;
                } else {
                    if key == "rules" {
                        meta.rules = Some(multiline_buf.clone());
                    }
                    current_multiline_key = None;
                    multiline_buf.clear();
                }
            }

            if let Some((k, v)) = trimmed.split_once(':') {
                let key = k.trim();
                let val = v.trim();
                if val == "|" {
                    current_multiline_key = Some(key.to_string());
                    multiline_buf.clear();
                } else {
                    let clean_val = val.trim_matches('"').trim_matches('\'').to_string();
                    match key {
                        "title" => meta.title = Some(clean_val),
                        "version" => meta.version = Some(clean_val),
                        "base_path" => meta.base_path = Some(clean_val),
                        "rules" => meta.rules = Some(clean_val),
                        _ => {}
                    }
                }
            }
        }

        if let Some(ref key) = current_multiline_key
            && key == "rules"
        {
            meta.rules = Some(multiline_buf);
        }

        meta
    }

    /// 在代码块中识别首个独立的 `@mock-when` 或 `@mock-default` 指令行，
    /// 将代码块一刀切分为 `(request_part, variants_part)`。（委托 MockVariantParser 统一组件）
    pub fn split_mock_variants(block_content: &str) -> Option<(String, String)> {
        crate::parser::mock_variant::MockVariantParser::split_mock_variants(block_content)
    }

    /// 解析切分出的 Mock 变体部分（委托 MockVariantParser 统一组件）
    pub fn parse_mock_variants_part(variants_content: &str) -> Vec<ParsedMockVariant> {
        crate::parser::mock_variant::MockVariantParser::parse_mock_variants_part(variants_content)
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

                // 代码块开始：info string 只取首个 token 作为语言标签，
                // ```http title="demo" 这类带属性的围栏此前因整串比较被忽略
                Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(lang))) => {
                    let lang_token = lang.split_whitespace().next().unwrap_or("").to_lowercase();
                    if lang_token == "http" || lang_token == "rest" {
                        in_code_block = true;
                        current_code.clear();
                    }
                }

                // 代码块结束
                Event::End(TagEnd::CodeBlock) if in_code_block => {
                    blocks.push(ExtractedCodeBlock {
                        content: current_code.clone(),
                        preceding_header: current_header.clone(),
                        // WP-P2a：真实行号由 assign_fence_lines 回填（先占位）
                        line_number: 1,
                    });

                    in_code_block = false;
                    current_code.clear();
                }

                _ => {}
            }
        }

        assign_fence_lines(content, &mut blocks);
        blocks
    }
}

/// WP-P2a：扫描原文围栏开行，按出现顺序为 http 围栏块回填真实 1-based 行号
/// （pulldown-cmark 事件流不携带行号）。围栏标记 ```/~~~ 均支持；
/// 计数与块数不一致（畸形文档/缩进围栏误判）时放弃回填，保持占位值——
/// 宁可退化不可误导。
fn assign_fence_lines(content: &str, blocks: &mut [ExtractedCodeBlock]) {
    let mut opens: Vec<usize> = Vec::new();
    let mut in_fence = false;
    let mut marker = "";
    for (idx, line) in (1..).zip(content.lines()) {
        let trimmed = line.trim_start();
        if !in_fence {
            if trimmed.starts_with("```") {
                in_fence = true;
                marker = "```";
                opens.push(idx);
            } else if trimmed.starts_with("~~~") {
                in_fence = true;
                marker = "~~~";
                opens.push(idx);
            }
        } else if trimmed.starts_with(marker) {
            in_fence = false;
        }
    }
    if opens.len() == blocks.len() {
        for (block, line) in blocks.iter_mut().zip(opens) {
            block.line_number = line;
        }
    }
}

#[derive(Debug)]
struct ExtractedCodeBlock {
    content: String,
    preceding_header: Option<String>,
    line_number: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fence_info_string_with_attributes_is_recognized() {
        // 回归：```http title="demo" 这类带属性的 info string 此前被整串比较忽略
        let content = "# Doc\n\n```http title=\"demo\" linenos\nGET https://api.example.com/attr\n```\n\n```HTTP\nGET https://api.example.com/upper\n```\n\n```rest\nGET https://api.example.com/rest\n```\n\n```json\n{\"not\": \"http\"}\n```\n";
        let parsed = MarkdownFileParser::parse_content(content).unwrap();
        let urls: Vec<&str> = parsed.requests.iter().map(|r| r.url.as_str()).collect();
        assert_eq!(
            urls,
            vec![
                "https://api.example.com/attr",
                "https://api.example.com/upper",
                "https://api.example.com/rest",
            ]
        );
    }

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

    #[test]
    fn test_frontmatter_toml_parsing() {
        // 1. 验证 --- 定界符包裹的 TOML
        let content_dash = r#"---
title = "User Order API"
version = "2.1.0"
base_path = "/api/v2"
rules = "all amounts in cents"
---

# User Order API

```http
GET /orders
```
"#;
        let parsed = MarkdownFileParser::parse_content(content_dash).unwrap();
        assert_eq!(parsed.metadata.title.as_deref(), Some("User Order API"));
        assert_eq!(parsed.metadata.version.as_deref(), Some("2.1.0"));
        assert_eq!(parsed.metadata.base_path.as_deref(), Some("/api/v2"));
        assert_eq!(
            parsed.metadata.rules.as_deref(),
            Some("all amounts in cents")
        );
        assert_eq!(parsed.requests.len(), 1);
        assert_eq!(parsed.requests[0].base_path.as_deref(), Some("/api/v2"));

        // 2. 验证 +++ 定界符包裹的 TOML
        let content_plus = r#"+++
title = "Zola Style API"
base_path = "/v1"
+++

```http
GET /items
```
"#;
        let parsed_plus = MarkdownFileParser::parse_content(content_plus).unwrap();
        assert_eq!(
            parsed_plus.metadata.title.as_deref(),
            Some("Zola Style API")
        );
        assert_eq!(parsed_plus.metadata.base_path.as_deref(), Some("/v1"));
        assert_eq!(parsed_plus.requests.len(), 1);
        assert_eq!(parsed_plus.requests[0].base_path.as_deref(), Some("/v1"));
    }
}
