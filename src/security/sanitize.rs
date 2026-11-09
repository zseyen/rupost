//! 统一 HTML 与 XSS 清洗组件 (HTML Sanitizer Component)
//!
//! 提供统一、零拷贝分配的高性能 5 实体转义 (`&`, `<`, `>`, `"`, `'`)，
//! 消除跨模块与前后端转义规则不一致的隐患。

use std::borrow::Cow;

pub struct HtmlSanitizer;

impl HtmlSanitizer {
    /// 对不可信字符串执行严格的 HTML 5 实体转义
    pub fn escape(raw: &str) -> Cow<'_, str> {
        let needs_escape = raw
            .chars()
            .any(|c| matches!(c, '&' | '<' | '>' | '"' | '\''));

        if !needs_escape {
            return Cow::Borrowed(raw);
        }

        let mut escaped = String::with_capacity(raw.len() + 16);
        for c in raw.chars() {
            match c {
                '&' => escaped.push_str("&amp;"),
                '<' => escaped.push_str("&lt;"),
                '>' => escaped.push_str("&gt;"),
                '"' => escaped.push_str("&quot;"),
                '\'' => escaped.push_str("&#39;"),
                _ => escaped.push(c),
            }
        }
        Cow::Owned(escaped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_sanitizer_escape() {
        assert_eq!(
            HtmlSanitizer::escape("<script>alert('xss & \"attack\"')</script>"),
            "&lt;script&gt;alert(&#39;xss &amp; &quot;attack&quot;&#39;)&lt;/script&gt;"
        );

        // 验证无特殊字符时零拷贝
        let safe = "plain text 123";
        assert!(matches!(HtmlSanitizer::escape(safe), Cow::Borrowed(_)));
    }
}
