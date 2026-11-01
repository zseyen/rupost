/// 从 LLM 原始文本中提取 JSON 字符串块（自动剥离 ```json 围栏及首尾非 JSON 噪音）。
/// 纯字节定位（`find`/`rfind` 返回 ASCII 边界），不引入 UTF-8 截断风险。
pub fn extract_json_block(raw: &str) -> &str {
    let trimmed = raw.trim();

    // 1. 若包含 ``` 围栏，优先提取第一个围栏内部的内容（跳过语言标识行）
    if let Some(fence_start) = trimmed.find("```") {
        let after_fence = &trimmed[fence_start + 3..];
        let content_start = after_fence.find('\n').map(|i| i + 1).unwrap_or(0);
        let inner = &after_fence[content_start..];
        if let Some(fence_end) = inner.find("```") {
            return inner[..fence_end].trim();
        }
    }

    // 2. 否则定位最外层的 `{ ... }` 或 `[ ... ]`
    let first_brace = trimmed.find('{');
    let first_bracket = trimmed.find('[');

    match (first_brace, first_bracket) {
        (Some(b), Some(k)) if b < k => {
            if let Some(end) = trimmed.rfind('}')
                && end >= b
            {
                return &trimmed[b..=end];
            }
        }
        (Some(b), None) => {
            if let Some(end) = trimmed.rfind('}')
                && end >= b
            {
                return &trimmed[b..=end];
            }
        }
        (None, Some(k)) | (Some(_), Some(k)) => {
            if let Some(end) = trimmed.rfind(']')
                && end >= k
            {
                return &trimmed[k..=end];
            }
        }
        (None, None) => {}
    }

    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_block_from_markdown_fence() {
        let raw =
            "Here is the analysis:\n```json\n{\"root_cause\": \"401\"}\n```\nHope this helps!";
        assert_eq!(extract_json_block(raw), r#"{"root_cause": "401"}"#);
    }

    #[test]
    fn test_extract_json_block_from_plain_fence() {
        let raw = "```\n[1, 2]\n```";
        assert_eq!(extract_json_block(raw), "[1, 2]");
    }

    #[test]
    fn test_extract_json_block_from_plain_text_with_prefix() {
        let raw = "Sure! [{\"category\": \"Auth\"}] is the result.";
        assert_eq!(extract_json_block(raw), r#"[{"category": "Auth"}]"#);
    }

    #[test]
    fn test_extract_json_block_bare_json_passthrough() {
        assert_eq!(extract_json_block("{\"a\": 1}"), "{\"a\": 1}");
    }

    #[test]
    fn test_extract_json_block_no_json_returns_trimmed() {
        assert_eq!(extract_json_block("  not json at all  "), "not json at all");
    }
}
