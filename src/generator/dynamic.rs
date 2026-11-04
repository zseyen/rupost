/// T1：历史中已解析的动态值按形状反向模板化（PRD `2026-11-04-p1-batch-*.md` §三 T1）。
/// 产物为 rupost 动态内置变量（`{{$uuid}}` 等，resolver 解析双花括号内 `$` 前缀），
/// 使生成的用例重跑时语义正确（新 UUID / 新时间戳），而非固化字面量。
/// 管线优先级（PRD D-序）：敏感键凭据模板化 > T1 形状模板化——敏感键值不进本扫描器。
use regex::Regex;
use std::sync::OnceLock;

/// 可映射到 rupost 动态内置变量的值形状
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DynamicShape {
    /// UUID v4 → `{{$uuid}}`
    Uuid,
    /// ISO8601 时间串 → `{{$timestamp_iso}}`
    TimestampIso,
    /// 13 位 epoch 毫秒 → `{{$timestamp_ms}}`
    TimestampMs,
    /// 10 位 epoch 秒 → `{{$timestamp}}`
    TimestampSecs,
}

fn uuid_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")
            .expect("valid uuid regex")
    })
}

fn iso_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})$")
            .expect("valid iso8601 regex")
    })
}

/// 识别值的动态形状（整值精确匹配；带前缀/后缀的复合值 v1 不识别）
pub fn detect_shape(value: &str) -> Option<DynamicShape> {
    if uuid_re().is_match(value) {
        Some(DynamicShape::Uuid)
    } else if iso_re().is_match(value) {
        Some(DynamicShape::TimestampIso)
    } else if value.len() == 13 && value.chars().all(|c| c.is_ascii_digit()) {
        Some(DynamicShape::TimestampMs)
    } else if value.len() == 10 && value.chars().all(|c| c.is_ascii_digit()) {
        Some(DynamicShape::TimestampSecs)
    } else {
        None
    }
}

/// 形状 → 动态内置变量占位符（`{{...}}` 双花括号形态，resolver 可解析）
pub fn template_value(value: &str) -> Option<String> {
    let placeholder = match detect_shape(value)? {
        DynamicShape::Uuid => "{{$uuid}}",
        DynamicShape::TimestampIso => "{{$timestamp_iso}}",
        DynamicShape::TimestampMs => "{{$timestamp_ms}}",
        DynamicShape::TimestampSecs => "{{$timestamp}}",
    };
    Some(placeholder.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T1 决策表：四形状命中 + 普通短值不误伤 + 复合值不识别
    #[test]
    fn test_detect_shape_decision_table() {
        // UUID v4（含大写与合法变体位）
        assert_eq!(
            detect_shape("8f9c2d55-1a2b-4c3d-9e4f-5a6b7c8d9e0f"),
            Some(DynamicShape::Uuid)
        );
        assert_eq!(
            detect_shape("550E8400-E29B-41D4-A716-446655440000"),
            Some(DynamicShape::Uuid)
        );
        // 非 v4 变体位不误伤（v1 形状位固定为 4）
        assert_eq!(detect_shape("550e8400-e29b-11d4-a716-446655440000"), None);
        // ISO8601（秒/毫秒/时区偏移）
        assert_eq!(
            detect_shape("2026-11-04T00:00:00Z"),
            Some(DynamicShape::TimestampIso)
        );
        assert_eq!(
            detect_shape("2026-11-04T00:00:00.123456+08:00"),
            Some(DynamicShape::TimestampIso)
        );
        // epoch 毫秒（13 位）/ 秒（10 位）
        assert_eq!(
            detect_shape("1772505600123"),
            Some(DynamicShape::TimestampMs)
        );
        assert_eq!(
            detect_shape("1772505600"),
            Some(DynamicShape::TimestampSecs)
        );
        // 普通短值/非全数字/12 位（无对应形状）不误伤
        assert_eq!(detect_shape("pro"), None);
        assert_eq!(detect_shape("12345"), None);
        assert_eq!(detect_shape("123456789012"), None);
        assert_eq!(detect_shape("1772505600abc"), None);
        // 复合值（Bearer 前缀）不识别
        assert_eq!(
            detect_shape("Bearer 8f9c2d55-1a2b-4c3d-9e4f-5a6b7c8d9e0f"),
            None
        );
    }

    /// 产物必须是 resolver 可解析的双花括号动态变量
    #[test]
    fn test_template_value_produces_braced_builtins() {
        assert_eq!(
            template_value("8f9c2d55-1a2b-4c3d-9e4f-5a6b7c8d9e0f"),
            Some("{{$uuid}}".to_string())
        );
        assert_eq!(
            template_value("2026-11-04T00:00:00Z"),
            Some("{{$timestamp_iso}}".to_string())
        );
        assert_eq!(
            template_value("1772505600123"),
            Some("{{$timestamp_ms}}".to_string())
        );
        assert_eq!(
            template_value("1772505600"),
            Some("{{$timestamp}}".to_string())
        );
        assert_eq!(template_value("hello"), None);
    }
}
