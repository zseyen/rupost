//! Mock 变体切分与解析组件 (Mock Variant Parser Component)
//!
//! 负责从 HTTP 代码块中提取 `@mock-when` 与 `@mock-default` 变体规则，
//! 供 Markdown 与纯 HTTP 解析器 100% 复用，彻底消除解析逻辑重复并打通双角色支持。

use crate::parser::parse_http_header;
use crate::parser::types::ParsedMockVariant;

#[derive(Debug, PartialEq, Eq)]
enum ParseState {
    VariantHeaders,
    VariantBody,
}

/// 独立的 Mock 变体解析器
pub struct MockVariantParser;

impl MockVariantParser {
    /// 切分测试请求部分与 Mock 变体部分
    pub fn split_mock_variants(content: &str) -> Option<(String, String)> {
        let mut req_lines = Vec::new();
        let mut var_lines = Vec::new();
        let mut in_variants = false;

        for line in content.lines() {
            let trimmed = line.trim();

            if !in_variants {
                let directive = if let Some(s) = trimmed.strip_prefix("//") {
                    s.trim()
                } else if let Some(s) = trimmed.strip_prefix('#') {
                    s.trim()
                } else {
                    trimmed
                };

                let is_mock_directive = directive.starts_with("@mock-when")
                    || directive == "@mock-default"
                    || directive.starts_with("@mock-default ");

                if is_mock_directive {
                    in_variants = true;
                    var_lines.push(line);
                } else {
                    req_lines.push(line);
                }
            } else {
                var_lines.push(line);
            }
        }

        if in_variants {
            Some((req_lines.join("\n"), var_lines.join("\n")))
        } else {
            None
        }
    }

    /// 解析切分出的 Mock 变体内容
    pub fn parse_mock_variants_part(variants_content: &str) -> Vec<ParsedMockVariant> {
        let mut variants = Vec::new();
        let mut current_variant: Option<ParsedMockVariant> = None;
        let mut current_body_lines: Vec<&str> = Vec::new();
        let mut state = ParseState::VariantHeaders;

        for line in variants_content.lines() {
            let trimmed = line.trim();
            let directive = if let Some(s) = trimmed.strip_prefix("//") {
                s.trim()
            } else if let Some(s) = trimmed.strip_prefix('#') {
                s.trim()
            } else {
                trimmed
            };

            let is_mock_directive = directive.starts_with("@mock-when")
                || directive == "@mock-default"
                || directive.starts_with("@mock-default ");

            if is_mock_directive {
                if let Some(mut var) = current_variant.take() {
                    let body_str = current_body_lines.join("\n").trim().to_string();
                    var.body = if body_str.is_empty() {
                        None
                    } else {
                        Some(body_str)
                    };
                    variants.push(var);
                    current_body_lines.clear();
                }

                if let Some(stripped) = directive.strip_prefix("@mock-when") {
                    current_variant = Some(ParsedMockVariant {
                        condition_expr: Some(stripped.trim().to_string()),
                        status: 200,
                        headers: Vec::new(),
                        body: None,
                    });
                } else {
                    current_variant = Some(ParsedMockVariant {
                        condition_expr: None,
                        status: 200,
                        headers: Vec::new(),
                        body: None,
                    });
                }
                state = ParseState::VariantHeaders;
                continue;
            }

            match state {
                ParseState::VariantHeaders => {
                    if trimmed.is_empty() {
                        state = ParseState::VariantBody;
                    } else if trimmed.starts_with("HTTP/1.1") || trimmed.starts_with("HTTP/2") {
                        let parts: Vec<&str> = trimmed.split_whitespace().collect();
                        if parts.len() >= 2
                            && let Ok(code) = parts[1].parse::<u16>()
                            && let Some(var) = current_variant.as_mut()
                        {
                            var.status = code;
                        }
                    } else if let Some((k, v)) = parse_http_header(trimmed)
                        && let Some(var) = current_variant.as_mut()
                    {
                        var.headers.push((k.to_string(), v.to_string()));
                    }
                }
                ParseState::VariantBody => {
                    current_body_lines.push(line);
                }
            }
        }

        if let Some(mut var) = current_variant {
            let body_str = current_body_lines.join("\n").trim().to_string();
            var.body = if body_str.is_empty() {
                None
            } else {
                Some(body_str)
            };
            variants.push(var);
        }

        variants
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_and_parse_mock_variants() {
        let block = r#"GET /api/v1/user
Accept: application/json

@mock-when header.X-Role == admin
HTTP/1.1 200 OK
Content-Type: application/json

{"role": "admin"}

@mock-default
HTTP/1.1 403 Forbidden
Content-Type: application/json

{"error": "forbidden"}
"#;

        let (req_part, var_part) = MockVariantParser::split_mock_variants(block).unwrap();
        assert!(req_part.contains("GET /api/v1/user"));
        assert!(!req_part.contains("@mock-when"));

        let variants = MockVariantParser::parse_mock_variants_part(&var_part);
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].status, 200);
        assert_eq!(
            variants[0].condition_expr.as_deref(),
            Some("header.X-Role == admin")
        );
        assert_eq!(variants[1].status, 403);
        assert!(variants[1].condition_expr.is_none());
    }
}
