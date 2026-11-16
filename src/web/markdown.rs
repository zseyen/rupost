use crate::parser::http_file::HttpFileParser;
use crate::parser::markdown_file::MarkdownFileParser;
use crate::web::http_parse::parsed_request_to_card_dto;
use crate::web::types::{HttpCardDto, RenderMarkdownResponse};
use pulldown_cmark::{CodeBlockKind, CowStr, Event, Options, Parser, Tag, TagEnd, html};

/// 转义 HTML 文本与属性中的特殊字符，防止 XSS 注入
fn escape_html_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// 校验链接或图片 URL 协议安全性（仅放行安全协议与相对路径，拦截 javascript:/data:/vbscript:）
fn is_safe_url(url: &str) -> bool {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return true;
    }
    // 剥离控制字符/空白后检查 scheme，防止 "java\tscript:" 绕过
    let normalized: String = trimmed
        .chars()
        .filter(|c| !c.is_control() && !c.is_whitespace())
        .collect();
    let lower = normalized.to_ascii_lowercase();

    if lower.starts_with("//") {
        return false;
    }

    if lower.starts_with('#')
        || lower.starts_with('/')
        || lower.starts_with("./")
        || lower.starts_with("../")
    {
        return true;
    }

    if let Some(colon_pos) = lower.find(':') {
        let scheme = &lower[..colon_pos];
        // 若冒号前包含路径分隔符或查询符，属于相对路径而非 scheme
        if scheme.contains('/') || scheme.contains('?') || scheme.contains('#') {
            return true;
        }
        matches!(scheme, "http" | "https" | "mailto")
    } else {
        true
    }
}

/// 构造安全链接/图片目标地址
fn sanitize_url(url: CowStr<'_>) -> CowStr<'_> {
    if is_safe_url(&url) {
        url
    } else {
        CowStr::Borrowed("#")
    }
}

/// 将 Markdown 文本渲染为安全的 HTML，并提取可执行 HTTP 代码块
pub fn render_markdown_to_html(content: &str) -> RenderMarkdownResponse {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
    options.insert(Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS);

    let parser = Parser::new_ext(content, options);
    let mut executable_blocks = Vec::new();
    let mut transformed_events: Vec<Event<'_>> = Vec::new();

    let mut in_metadata = false;
    let mut in_code_block = false;
    let mut current_code_lang = String::new();
    let mut current_code_body = String::new();

    for event in parser {
        if in_metadata {
            if matches!(event, Event::End(TagEnd::MetadataBlock(_))) {
                in_metadata = false;
            }
            continue;
        }

        if in_code_block {
            match event {
                Event::Text(text) => {
                    current_code_body.push_str(&text);
                }
                Event::End(TagEnd::CodeBlock) => {
                    let html_block = build_code_block_html(
                        &current_code_lang,
                        &current_code_body,
                        &mut executable_blocks,
                    );
                    transformed_events
                        .push(Event::Html(CowStr::Boxed(html_block.into_boxed_str())));
                    in_code_block = false;
                    current_code_lang.clear();
                    current_code_body.clear();
                }
                _ => {}
            }
            continue;
        }

        match event {
            Event::Start(Tag::MetadataBlock(_)) => {
                in_metadata = true;
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                in_code_block = true;
                current_code_body.clear();
                current_code_lang = match kind {
                    CodeBlockKind::Fenced(lang) => {
                        lang.split_whitespace().next().unwrap_or("").to_string()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
            }
            // XSS 防护：将文档内手写的原生 HTML 标签降级为转义纯文本
            Event::Html(raw) | Event::InlineHtml(raw) => {
                transformed_events.push(Event::Text(raw));
            }
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                transformed_events.push(Event::Start(Tag::Link {
                    link_type,
                    dest_url: sanitize_url(dest_url),
                    title,
                    id,
                }));
            }
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                transformed_events.push(Event::Start(Tag::Image {
                    link_type,
                    dest_url: sanitize_url(dest_url),
                    title,
                    id,
                }));
            }
            other => transformed_events.push(other),
        }
    }

    let mut html_output = String::new();
    html::push_html(&mut html_output, transformed_events.into_iter());

    let executable_cards: Vec<HttpCardDto> = executable_blocks
        .iter()
        .filter_map(|raw| {
            HttpFileParser::parse_request_block(raw, 1)
                .ok()
                .flatten()
                .map(|req| parsed_request_to_card_dto(&req, raw.clone()))
        })
        .collect();

    RenderMarkdownResponse {
        html: html_output,
        executable_blocks,
        executable_cards,
    }
}

fn build_code_block_html(lang: &str, code: &str, executable_blocks: &mut Vec<String>) -> String {
    let escaped_code = escape_html_text(code);
    let escaped_lang = escape_html_text(&lang.to_ascii_uppercase());

    if MarkdownFileParser::is_executable_http_lang(lang) {
        let block_idx = executable_blocks.len();
        executable_blocks.push(code.to_string());
        format!(
            "<div class=\"md-code-block md-http-block\">\
               <div class=\"md-code-header\">\
                 <span class=\"md-code-lang\">{escaped_lang}</span>\
                 <button type=\"button\" class=\"btn btn-accent btn-sm md-run-btn\" data-block-index=\"{block_idx}\">&gt; Run</button>\
               </div>\
               <pre><code>{escaped_code}</code></pre>\
             </div>"
        )
    } else if !lang.is_empty() {
        format!(
            "<div class=\"md-code-block\">\
               <div class=\"md-code-header\">\
                 <span class=\"md-code-lang\">{escaped_lang}</span>\
               </div>\
               <pre><code>{escaped_code}</code></pre>\
             </div>"
        )
    } else {
        format!(
            "<div class=\"md-code-block\">\
               <pre><code>{escaped_code}</code></pre>\
             </div>"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_markdown_headings_tables_and_tasklists() {
        let md = "\
# Rupost Guide

> Note with **bold** and `inline`

| Mode | Command |
| :--- | :--- |
| CLI | `rupost test` |

- [x] Done task
- [ ] Pending task
";
        let res = render_markdown_to_html(md);
        assert!(res.html.contains("<h1>Rupost Guide</h1>"));
        assert!(res.html.contains("<blockquote>"));
        assert!(res.html.contains("<strong>bold</strong>"));
        assert!(res.html.contains("<code>inline</code>"));
        assert!(res.html.contains("<table>"));
        assert!(res.html.contains(">Mode</th>"));
        assert!(res.html.contains("type=\"checkbox\""));
        assert!(res.executable_blocks.is_empty());
    }

    #[test]
    fn test_render_markdown_http_block_injects_run_button() {
        let md = "\
## Login API

```http title=\"login\"
POST https://httpbin.org/post
Content-Type: application/json

{\"ok\": true}
```

```bash
echo \"hello\"
```
";
        let res = render_markdown_to_html(md);
        assert_eq!(res.executable_blocks.len(), 1);
        assert!(res.executable_blocks[0].contains("POST https://httpbin.org/post"));
        assert!(
            res.html
                .contains("class=\"btn btn-accent btn-sm md-run-btn\"")
        );
        assert!(res.html.contains("data-block-index=\"0\""));
        assert!(
            res.html
                .contains("<span class=\"md-code-lang\">HTTP</span>")
        );
        // bash block should show language header without run button
        assert!(
            res.html
                .contains("<span class=\"md-code-lang\">BASH</span>")
        );
        assert_eq!(res.html.matches("md-run-btn").count(), 1);
    }

    #[test]
    fn test_render_markdown_xss_sanitization() {
        let md = "\
<script>alert('xss')</script>

Inline <img src=x onerror=alert(1)> html

[Bad Link](javascript:alert(1))
[Tab Bypass](java\tscript:alert(1))
[Proto Relative](//evil.com/payload)
[Data Link](data:text/html,<script>alert(1)</script>)
[Safe Link](https://example.com/docs?a=1)
[Relative Link](doc/guides/quickstart.md)
";
        let res = render_markdown_to_html(md);
        assert!(!res.html.contains("<script>"));
        assert!(!res.html.contains("<img"));
        assert!(res.html.contains("&lt;img src=x onerror=alert(1)&gt;"));
        assert!(!res.html.contains("javascript:"));
        assert!(!res.html.contains("//evil.com"));
        assert!(!res.html.contains("data:text/html"));
        assert!(
            res.html
                .contains("&lt;script&gt;alert('xss')&lt;/script&gt;")
        );
        assert!(res.html.contains("href=\"https://example.com/docs?a=1\""));
        assert!(res.html.contains("href=\"doc/guides/quickstart.md\""));
    }

    #[test]
    fn test_render_markdown_strips_frontmatter() {
        let md = "\
+++
base_path = \"https://api.example.com\"
+++

# API Docs
";
        let res = render_markdown_to_html(md);
        assert!(!res.html.contains("base_path"));
        assert!(res.html.contains("<h1>API Docs</h1>"));
    }
}
