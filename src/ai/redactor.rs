//! `security::redact` 的 AI 表面垫片（WP 审查批四：原语全部下沉 security，
//! 本文件仅保留路径兼容 re-export——ai 内调用方零改动；report 已切换直连）。
pub use crate::security::redact::{
    DEFAULT_SNIPPET_MAX_BYTES, assertion_targets_sensitive_key, is_sensitive_key,
    mask_inline_secrets, mask_pii, mask_urls, order_secret_values, redact_and_truncate_body,
    redact_assertion_value, redact_explicit_secrets, redact_explicit_secrets_ordered,
    redact_free_text, redact_header_value, redact_json_value, redact_url, truncate_chars,
    truncate_utf8_safe,
};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

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

    #[test]
    fn test_redact_and_truncate_body_pii_and_numeric_parity() {
        let raw = r#"{
            "password": "pw1",
            "passwd": "pw2",
            "secret": "sec3",
            "token": "tok4",
            "access_token": "at5",
            "phone": 13812345678,
            "mobile": 13900001111,
            "id_card": 110101199001011234,
            "nested": {
                "phone": "13712345678",
                "id_card": "11010119900101123X",
                "primary_key": 7,
                "timestamp_ms": 1731396798000,
                "design": "clean",
                "signal": "ready"
            }
        }"#;
        let ai_out = redact_and_truncate_body(raw, 4096);
        let exp_out = crate::security::redact::mask_body_secrets(raw);
        let ai_val: Value = serde_json::from_str(&ai_out).unwrap();
        let exp_val: Value = serde_json::from_str(&exp_out).unwrap();
        assert_eq!(
            ai_val, exp_val,
            "AI redactor must match exporter mask_body_secrets"
        );

        assert_eq!(ai_val["passwd"], "[REDACTED]");
        assert_eq!(ai_val["phone"], "***");
        assert_eq!(ai_val["mobile"], "***");
        assert_eq!(ai_val["id_card"], "***");
        assert_eq!(ai_val["nested"]["phone"], "***");
        assert_eq!(ai_val["nested"]["id_card"], "***");
        assert_eq!(ai_val["nested"]["primary_key"], 7);
        assert_eq!(ai_val["nested"]["timestamp_ms"], 1731396798000_i64);
        assert_eq!(ai_val["nested"]["design"], "clean");
        assert_eq!(ai_val["nested"]["signal"], "ready");

        // 非 JSON 文本也必须同步掩码扩展后的内联密钥与 PII
        let text_out = redact_and_truncate_body(
            "X-Sign: deadbeef Set-Cookie: sid=xyz123; phone 13812345678 id 11010119900101123X",
            4096,
        );
        assert!(!text_out.contains("deadbeef"), "{text_out}");
        assert!(!text_out.contains("xyz123"), "{text_out}");
        assert!(!text_out.contains("13812345678"), "{text_out}");
        assert!(!text_out.contains("11010119900101123X"), "{text_out}");
    }
}
