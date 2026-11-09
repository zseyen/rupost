//! 文本处理与安全截断通用工具（DRY：收敛全库散落的 UTF-8 与字符截断实现）

/// 在 UTF-8 字符边界安全截断文本（按字节上限），超出时追加指定省略符。
pub fn truncate_utf8_bytes(text: &str, max_bytes: usize, ellipsis: &str) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut end = max_bytes.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", &text[..end], ellipsis)
}

/// 按 UTF-8 字符数截断字符串，超出时追加指定省略符。
pub fn truncate_chars(s: &str, max_chars: usize, ellipsis: &str) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let prefix: String = s.chars().take(max_chars).collect();
        format!("{prefix}{ellipsis}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_utf8_bytes_multibyte_safety() {
        let chinese = "错误原因定位";
        // "错" 3字节, "误" 3字节, "原" 3字节, "因" 3字节
        // 若切在 10 字节处，应安全回退到 9 字节（3 个完整字符）
        let truncated = truncate_utf8_bytes(chinese, 10, "…");
        assert_eq!(truncated, "错误原…");
    }

    #[test]
    fn test_truncate_chars_table() {
        let cases = [
            ("short", 10, "…", "short"),
            ("exact12345", 10, "…", "exact12345"),
            ("longer-than-limit", 6, "…", "longer…"),
            ("你好世界欢迎使用", 4, "...", "你好世界..."),
            ("abc", 2, "", "ab"),
        ];

        for (input, limit, ellipsis, expected) in cases {
            assert_eq!(
                truncate_chars(input, limit, ellipsis),
                expected,
                "input: {input}"
            );
        }
    }
}
