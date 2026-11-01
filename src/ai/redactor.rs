pub use crate::exporter::redact::{is_sensitive_key, redact_header_value, redact_url};
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

/// 默认响应体片段字节上限（512 Bytes）
pub const DEFAULT_SNIPPET_MAX_BYTES: usize = 512;

fn url_regex() -> &'static Regex {
    static URL_RE: OnceLock<Regex> = OnceLock::new();
    URL_RE.get_or_init(|| Regex::new(r#"https?://[^\s"'<>`)]+"#).expect("valid url regex"))
}

/// 自由文本脱敏（Gap Review P1-1）：错误消息等散文中内嵌的 URL 走 `redact_url`，
/// 掩掉 query 中的敏感键值——reqwest 错误 Display 自带完整原始 URL，是密钥外泄主通道。
pub fn redact_free_text(text: &str) -> String {
    url_regex()
        .replace_all(text, |caps: &regex::Captures| redact_url(&caps[0]))
        .to_string()
}

/// 将文本中的 URL 整体替换为 `<url>` 占位符（用于去重签名：同端点不同实例同签名）
pub fn mask_urls(text: &str) -> String {
    url_regex().replace_all(text, "<url>").to_string()
}

/// 判断断言 raw 引用的字段是否命中敏感键（如 `body.access_token`、`headers.Authorization`）
pub fn assertion_targets_sensitive_key(raw: &str) -> bool {
    raw.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .any(is_sensitive_key)
}

/// 断言期望/实际/消息值的脱敏（Gap Review P1-2）：
/// - raw 命中敏感键 → 值整体 `[REDACTED]`（值可能源自响应体敏感字段，无法从值本身判断敏感性）；
/// - 否则仅做自由文本 URL 掩码 + 200 字符截断。
pub fn redact_assertion_value(raw: &str, value: Option<&str>) -> Option<String> {
    if assertion_targets_sensitive_key(raw) {
        value.map(|_| "[REDACTED]".to_string())
    } else {
        value.map(|s| truncate_utf8_safe(&redact_free_text(s), 200))
    }
}

/// 在 UTF-8 字符边界安全截断文本（按字节上限），超出时追加 `...[truncated]`。
/// 禁止裸切片（`&s[..n]` 会在多字节字符中间 panic），必须经 `is_char_boundary` 回退。
pub fn truncate_utf8_safe(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut end = max_bytes.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...[truncated]", &text[..end])
}

/// 按字符数截断错误消息，防止 AI 后端返回的超长错误体刷屏
pub fn truncate_chars(msg: &str, max_chars: usize) -> String {
    if msg.chars().count() <= max_chars {
        msg.to_string()
    } else {
        let truncated: String = msg.chars().take(max_chars).collect();
        format!("{truncated}...[truncated]")
    }
}

/// 递归遍历 JSON Value，将命中 `is_sensitive_key` 的对象字段值替换为 `"[REDACTED]"`（结构保留）
pub fn redact_json_value(val: &mut Value) {
    match val {
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                if is_sensitive_key(k) {
                    *v = Value::String("[REDACTED]".to_string());
                } else {
                    redact_json_value(v);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                redact_json_value(item);
            }
        }
        _ => {}
    }
}

/// 对响应体进行结构化脱敏并安全截断至 `max_bytes`：
/// 1. 若响应体可解析为 JSON，先递归掩码敏感键再序列化为紧凑字符串；
/// 2. 非 JSON 文本（HTML 错误页/纯文本）应用自由文本 URL 掩码（Gap Review P3：
///    错误页常含 `?token=` 形态的回跳链接），再参与截断；
/// 3. 在 UTF-8 字符边界处截断。
pub fn redact_and_truncate_body(body: &str, max_bytes: usize) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let sanitized = if (trimmed.starts_with('{') || trimmed.starts_with('['))
        && let Ok(mut json_val) = serde_json::from_str::<Value>(trimmed)
    {
        redact_json_value(&mut json_val);
        serde_json::to_string(&json_val).unwrap_or_else(|_| trimmed.to_string())
    } else {
        // 非 JSON（HTML/文本）：URL 掩码之外，兜底掩码内联键值凭据
        // （token=xxx / api_key: xxx —— snippet 上行 LLM 前的最后防线）
        mask_inline_secrets(&redact_free_text(trimmed))
    };

    truncate_utf8_safe(&sanitized, max_bytes)
}

/// 非 JSON 文本的内联键值凭据兜底掩码：`token=xxx` / `api_key: xxx` 形态
/// （键名保留、值替换为 [REDACTED]；值长度 ≥4 才掩，避免误伤短 token 字样）
fn mask_inline_secrets(text: &str) -> String {
    static INLINE_SECRET: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = INLINE_SECRET.get_or_init(|| {
        regex::Regex::new(
            // 值侧允许可选 scheme 前缀（Bearer/Basic/…），否则
            // "Authorization: Bearer eyJ…" 只掩到 "Bearer" 而 JWT 真值残留
            r#"(?i)(token|api[_-]?key|secret|password|passwd|authorization)\s*[=:]\s*(?:(?:bearer|basic|digest|token)\s+)?[^\s"'<>&,;}{]{4,}"#,
        )
        .expect("inline secret regex")
    });
    re.replace_all(text, "${1}=[REDACTED]").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_json_body_and_preserve_structure() {
        let raw = r#"{"user":"alice","access_token":"secret-jwt-123","nested":{"password":"pw","code":401}}"#;
        let out = redact_and_truncate_body(raw, 512);
        assert!(out.contains(r#""user":"alice""#));
        assert!(out.contains(r#""access_token":"[REDACTED]""#));
        assert!(out.contains(r#""password":"[REDACTED]""#));
        assert!(!out.contains("secret-jwt-123"));
        // 数值字段不受脱敏影响（结构保留）
        assert!(out.contains("401"));
    }

    #[test]
    fn test_sensitive_wordlist_expanded() {
        // Gap Review P3：词表扩充（private_key/access_key/credential/passphrase）
        let raw = r#"{"private_key":"pk-1","access_key":"ak-2","credentials":{"passphrase":"pp"}}"#;
        let out = redact_and_truncate_body(raw, 512);
        assert!(out.contains(r#""private_key":"[REDACTED]""#));
        assert!(out.contains(r#""access_key":"[REDACTED]""#));
        // "credentials" 键本身命中词表 → 整体掩码（正确行为，嵌套不再遍历）
        assert!(out.contains(r#""credentials":"[REDACTED]""#));
        assert!(!out.contains("pk-1"));
        assert!(!out.contains("ak-2"));
        assert!(!out.contains("pp"));
        // 误报检查：非敏感键不受影响（session 未入词表，session_note 保留）
        let ok = redact_and_truncate_body(r#"{"session_note":"x","username":"alice"}"#, 512);
        assert!(ok.contains(r#""session_note":"x""#));
        assert!(ok.contains(r#""username":"alice""#));
    }

    #[test]
    fn test_redact_json_array_and_nested_objects() {
        let raw = r#"{"users":[{"token":"t1","name":"a"},{"Password":"p2"}]}"#;
        let out = redact_and_truncate_body(raw, 512);
        assert!(out.contains(r#""token":"[REDACTED]""#));
        assert!(out.contains(r#""Password":"[REDACTED]""#));
        assert!(out.contains(r#""name":"a""#));
        assert!(!out.contains("t1"));
        assert!(!out.contains("p2"));
    }

    #[test]
    fn test_non_json_body_url_masked_with_truncation() {
        let raw = "plain error text with secret-jwt-123";
        // 非 JSON 文本不做键匹配（无 URL 时原样保留）
        let out = redact_and_truncate_body(raw, 512);
        assert_eq!(out, raw);
        // Gap Review P3：HTML/纯文本错误页中的 URL（常含回跳 token）也必须掩码
        let html = r#"<html>redirect to http://sso.local/callback?token=abc123&next=/home</html>"#;
        let out = redact_and_truncate_body(html, 512);
        assert!(!out.contains("abc123"), "URL query 密钥必须掩码: {out}");
        assert!(out.contains("token=%5BREDACTED%5D") || out.contains("token=[REDACTED]"));
        assert!(
            out.contains("redirect to http://sso.local/callback"),
            "非敏感部分保留"
        );
    }

    #[test]
    fn test_truncate_utf8_safe_multibyte_chinese() {
        // 每个汉字占 3 字节；截断点落在多字节字符中间时回退到字符边界
        let text = "错误原因分析：数据库连接超时";
        let truncated = truncate_utf8_safe(text, 10);
        assert_eq!(truncated, "错误原...[truncated]");
        // 短文本不截断
        assert_eq!(truncate_utf8_safe("短", 512), "短");
    }

    #[test]
    fn test_mask_inline_secrets_bearer_scheme_prefix() {
        // scheme 前缀（Bearer）随值一并掩码，JWT 真值不得残留
        let masked =
            super::mask_inline_secrets("Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.payload.sig");
        assert!(!masked.contains("eyJhbGci"), "JWT must be masked: {masked}");
        assert!(masked.contains("Authorization=[REDACTED]"), "{masked}");
    }

    #[test]
    fn test_mask_inline_secrets_short_value_and_non_secrets() {
        // 值 <4 字符不掩（明示取舍）；tokenize= 不构成 token= 锚点，不误伤
        assert_eq!(
            super::mask_inline_secrets("token=ab and tokenize=x"),
            "token=ab and tokenize=x"
        );
        // JSON 路径已由 redact_json_value 处理，此处只锁非 JSON 兜底行为
        assert_eq!(
            super::mask_inline_secrets("api_key: sk-abcdef123456"),
            "api_key=[REDACTED]"
        );
    }

    #[test]
    fn test_redaction_is_idempotent() {
        let raw = r#"{"token":"secret","data":{"api_key":"k2","note":"ok"}}"#;
        let once = redact_and_truncate_body(raw, 512);
        let twice = redact_and_truncate_body(&once, 512);
        assert_eq!(once, twice);
    }

    #[test]
    fn test_empty_body_returns_empty() {
        assert_eq!(redact_and_truncate_body("", 512), "");
        assert_eq!(redact_and_truncate_body("   \n", 512), "");
    }

    #[test]
    fn test_truncate_chars_counts_chars_not_bytes() {
        let msg = "错".repeat(600);
        let out = truncate_chars(&msg, 500);
        assert!(out.starts_with(&"错".repeat(500)));
        assert!(out.ends_with("...[truncated]"));
    }

    #[test]
    fn test_redact_free_text_masks_query_secrets_in_error_messages() {
        // Gap Review P1-1 复现路径：reqwest 错误 Display 自带完整原始 URL
        let err = "error sending request for url (http://127.0.0.1:59999/v1/users/1?api_key=super-secret-123)";
        let out = redact_free_text(err);
        assert!(!out.contains("super-secret-123"), "密钥必须被掩码: {out}");
        assert!(out.contains("api_key="), "键名保留便于诊断: {out}");
        assert!(out.contains("/v1/users/1"), "路径保留便于诊断: {out}");
    }

    #[test]
    fn test_mask_urls_stabilizes_signatures() {
        let a = "error sending request for url (http://api.local/v1/users/1?api_key=k1)";
        let b = "error sending request for url (http://api.local/v1/users/2?api_key=k2)";
        assert_eq!(mask_urls(a), mask_urls(b), "同模式不同实例的 URL 应归一");
        assert_eq!(mask_urls(a), "error sending request for url (<url>)");
    }

    #[test]
    fn test_assertion_targets_sensitive_key() {
        assert!(assertion_targets_sensitive_key(
            r#"body.access_token == "x""#
        ));
        assert!(assertion_targets_sensitive_key("body.data.token exists"));
        assert!(assertion_targets_sensitive_key(
            "headers.Authorization exists"
        ));
        assert!(!assertion_targets_sensitive_key("status == 200"));
        assert!(!assertion_targets_sensitive_key(
            r#"body.user.name == "alice""#
        ));
    }

    #[test]
    fn test_redact_assertion_value_masks_sensitive_and_free_text() {
        // 敏感键断言：值整体掩码（值源自响应体，无法从值判断敏感性）
        assert_eq!(
            redact_assertion_value(
                r#"body.access_token == "wrong""#,
                Some("expired-jwt-should-not-leak")
            ),
            Some("[REDACTED]".to_string())
        );
        // 非敏感断言：仅 URL 掩码 + 截断
        assert_eq!(
            redact_assertion_value("status == 200", Some("401")),
            Some("401".to_string())
        );
        assert_eq!(
            redact_assertion_value("body.msg contains x", Some("see http://h.com/?token=abc")),
            Some("see http://h.com/?token=%5BREDACTED%5D".to_string())
        );
        assert_eq!(redact_assertion_value("status == 200", None), None);
    }
}
