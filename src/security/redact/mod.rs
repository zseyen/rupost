//! 敏感凭据脱敏工具

pub use crate::security::redact_audit::{RedactionAuditEntry, RedactionAuditLog, RedactionSource};

/// 永远不可豁免的绝对硬键（按 ASCII 字母升序严格排列，以保证 binary_search 确定性生效）
pub const EXACT_KEYS: &[&str] = &[
    "api-key",
    "apikey",
    "authorization",
    "cookie",
    "proxy-authorization",
    "set-cookie",
    "x-api-key",
    "x-auth-token",
];

/// 易受子串误报的模糊词族（可被 ignore_keys 豁免）
pub const SUBSTRINGS: &[&str] = &[
    "access_key",
    "apikey",
    "api_key",
    "credential",
    "passphrase",
    "password",
    "private_key",
    "secret",
    "token",
];

/// 按 `_` 与 `-` 切词后匹配的敏感词根（避免 design/assign/signal/keyboard 误报，可被 ignore_keys 豁免）
/// 严格按 ASCII 字母升序排列，以保证 binary_search 确定性生效。
pub const SENSITIVE_TOKENS: &[&str] = &["key", "passwd", "sig", "sign", "signature"];

/// 内置默认豁免词表（按 ASCII 字母升序严格排列，解决分页游标等误报痛点）
pub const DEFAULT_IGNORE_KEYS: &[&str] = &[
    "cursor",
    "next_cursor",
    "next_page_token",
    "next_token",
    "page_token",
    "pagination_token",
    "primary_key",
];

#[inline]
pub(crate) fn is_hard_sensitive_key(key_str: &str) -> bool {
    EXACT_KEYS.binary_search(&key_str).is_ok() || key_str.starts_with("authorization")
}

#[inline]
pub(crate) fn matches_fuzzy_sensitive_key(key_str: &str) -> bool {
    SUBSTRINGS.iter().any(|&sub| key_str.contains(sub))
        || key_str
            .split(['_', '-'])
            .any(|tok| SENSITIVE_TOKENS.binary_search(&tok).is_ok())
}

/// 判断键名是否属于 PII 数值字段（如 phone / mobile / id_card），用于对 JSON Number 叶子打码
#[inline]
pub fn is_pii_key(key: &str) -> bool {
    let lower = key.trim().to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "phone"
            | "mobile"
            | "id_card"
            | "id-card"
            | "idcard"
            | "phone_number"
            | "phone_no"
            | "mobile_number"
            | "mobile_no"
            | "id_card_no"
            | "id_card_number"
            | "user_phone"
            | "user_mobile"
            | "contact_phone"
            | "contact_mobile"
    ) || lower
        .split(['_', '-'])
        .next_back()
        .is_some_and(|last| matches!(last, "phone" | "mobile" | "idcard"))
        || lower.ends_with("_id_card")
        || lower.ends_with("-id-card")
}

mod policy;
mod primitives;

pub use policy::RedactionPolicy;
pub use policy::is_sensitive_key;
pub use primitives::{
    DEFAULT_SNIPPET_MAX_BYTES, assertion_targets_sensitive_key, mask_body_secrets,
    mask_inline_secrets, mask_json_pii_value_in_place, mask_pii, mask_urls, order_secret_values,
    redact_and_truncate_body, redact_assertion_value, redact_explicit_secrets,
    redact_explicit_secrets_ordered, redact_free_text, redact_header_value, redact_json_value,
    redact_url, truncate_chars, truncate_utf8_safe,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_url_userinfo_password_masked() {
        assert_eq!(
            redact_url("https://user:supersecret@api.example.com/v1"),
            "https://user:%5BREDACTED%5D@api.example.com/v1"
        );
        // 空密码段无凭据可掩（url crate 归一化处理空密码），不 panic、不误掩即可
        let _ = redact_url("https://user:@api.example.com");
        // IPv6 host 形态不受影响
        assert!(redact_url("http://user:pass@[::1]:8080/v1").contains("%5BREDACTED%5D"));
    }

    #[test]
    fn test_redact_url_oauth_fragment_masked() {
        // OAuth implicit 流：fragment 携带 access_token
        let masked =
            redact_url("https://api.example.com/callback#access_token=eyJhbGciOiJIUzI1NiJ9");
        assert!(
            !masked.contains("eyJhbGci"),
            "fragment token must be masked: {masked}"
        );
        // 普通锚点（无敏感键名）不掩
        assert_eq!(
            redact_url("https://docs.example.com/guide#section-2"),
            "https://docs.example.com/guide#section-2"
        );
    }

    #[test]
    fn test_redact_url_fragment_key_value_reuse_and_no_false_positives() {
        // 1. 结构保真：敏感键脱敏，非敏感键保留
        let url = "https://api.example.com/callback#access_token=secret123&state=xyz_state";
        let masked = redact_url(url);
        assert!(!masked.contains("secret123"));
        assert!(
            masked.contains("access_token=%5BREDACTED%5D")
                || masked.contains("access_token=[REDACTED]")
        );
        assert!(masked.contains("state=xyz_state"));

        // 2. SPA 路由形态：带 ? 的 fragment
        let spa_url = "https://app.com/spa#/login?token=secret456&role=admin";
        let spa_masked = redact_url(spa_url);
        assert!(!spa_masked.contains("secret456"));
        assert!(
            spa_masked.contains("token=%5BREDACTED%5D") || spa_masked.contains("token=[REDACTED]")
        );
        assert!(spa_masked.contains("role=admin"));

        // 3. 避免过度脱敏误杀：包含 "key" 或 "access" 的非凭据普通文本锚点 100% 保留
        assert_eq!(
            redact_url("https://docs.example.com/guide#keyboard-shortcuts"),
            "https://docs.example.com/guide#keyboard-shortcuts"
        );
        assert_eq!(
            redact_url("https://ui.org/guide#accessibility"),
            "https://ui.org/guide#accessibility"
        );
        assert_eq!(
            redact_url("https://site.com/faq#hotkeys"),
            "https://site.com/faq#hotkeys"
        );

        // 4. 审计日志精确记录真实键名与 UrlFragment 来源
        let mut audit_log = RedactionAuditLog::new();
        let policy = RedactionPolicy::builtin();
        let _ = policy.redact_url_audited(url, Some(&mut audit_log));
        assert_eq!(audit_log.len(), 1);
        let entry = &audit_log.entries()[0];
        assert_eq!(entry.key, "access_token");
        assert_eq!(entry.source, RedactionSource::UrlFragment);
    }

    #[test]
    fn test_redact_url_query_unchanged_when_not_sensitive() {
        // 无敏感键时原样返回（无格式漂移）
        assert_eq!(
            redact_url("https://api.example.com/v1/users?page=2&limit=10"),
            "https://api.example.com/v1/users?page=2&limit=10"
        );
    }

    #[test]
    fn test_is_sensitive_key() {
        assert!(is_sensitive_key("Authorization"));
        assert!(is_sensitive_key("authorization"));
        assert!(is_sensitive_key("X-API-KEY"));
        assert!(is_sensitive_key("api_key"));
        assert!(is_sensitive_key("User-Token"));
        assert!(is_sensitive_key("Cookie"));
        assert!(!is_sensitive_key("Content-Type"));
        assert!(!is_sensitive_key("User-Agent"));
    }

    /// 表驱动决策表：验证硬键、模糊词族、以及内置忽略词表（误报治理）
    #[test]
    fn test_is_sensitive_key_decision_table() {
        // 常量表基本完整性验证
        assert!(EXACT_KEYS.contains(&"authorization"));
        assert!(EXACT_KEYS.contains(&"cookie"));
        assert!(SUBSTRINGS.contains(&"token"));
        assert!(DEFAULT_IGNORE_KEYS.contains(&"next_token"));
        assert!(DEFAULT_IGNORE_KEYS.contains(&"page_token"));

        // 决策表用例：(输入键, 期望是否敏感, 描述)
        let cases = [
            // 硬键（必须敏感）
            ("Authorization", true, "规范大写鉴权头"),
            ("authorization", true, "小写鉴权头"),
            ("  AUTHORIZATION  ", true, "带空白鉴权头"),
            ("Cookie", true, "Cookie 头"),
            ("x-api-key", true, "API Key 头"),
            ("proxy-authorization", true, "代理鉴权头"),
            // 模糊词族（敏感）
            ("my_secret_token", true, "含 secret 和 token"),
            ("user_password", true, "含 password"),
            ("private_key", true, "含 private_key"),
            ("github_token", true, "含 token"),
            // 内置误报治理白名单（必须豁免，不再被 token 误伤！）
            ("next_token", false, "分页游标 next_token"),
            ("page_token", false, "分页游标 page_token"),
            ("next_page_token", false, "分页游标 next_page_token"),
            ("cursor", false, "分页游标 cursor"),
            ("next_cursor", false, "分页游标 next_cursor"),
            ("pagination_token", false, "分页游标 pagination_token"),
            // 普通安全键（不敏感）
            ("Content-Type", false, "内容类型"),
            ("Accept", false, "接收类型"),
            ("User-Agent", false, "客户端代理"),
            ("Host", false, "域名主机"),
        ];

        for (input, expected, desc) in cases {
            assert_eq!(
                is_sensitive_key(input),
                expected,
                "键名 '{}' ({}) 判定不符合预期，期望: {}, 实际: {}",
                input,
                desc,
                expected,
                !expected
            );
        }
    }

    /// 审查批二 C-d：ignore_keys 豁免决策表——doc 承诺"豁免 SUBSTRINGS 模糊
    /// 匹配"，此前仅精确匹配（next_page_token_v2 类后缀键无法豁免）。
    /// 最终裁决：子串包含豁免（精确= 或 contains=豁免键）；硬键护栏前置不变。
    #[test]
    fn test_ignore_keys_word_root_exemption_decision_table() {
        let policy = RedactionPolicy::new([] as [&str; 0], ["next_page_token"]);

        assert!(!policy.is_sensitive("next_page_token"));
        assert!(!policy.is_sensitive("next_page_token_v2"));
        assert!(policy.is_sensitive("access_token"));
        assert!(policy.is_sensitive("Authorization"));
        assert!(!policy.is_sensitive("page.next_page_token.count"));
    }

    /// Step 2 Red 测试：RedactionPolicy 策略实体与自定义 extra_keys / ignore_keys
    #[test]
    fn test_redaction_policy_custom_extra_and_ignore() {
        let policy = RedactionPolicy::new(
            ["x-sentry-key", "fastly-key"],
            ["custom_token", "next_token"],
        );

        // 自定义 extra_keys 必须敏感
        assert!(policy.is_sensitive("X-Sentry-Key"));
        assert!(policy.is_sensitive("fastly-key"));

        // 自定义 ignore_keys 必须豁免（即使包含 token 子串）
        assert!(!policy.is_sensitive("custom_token"));
        assert!(!policy.is_sensitive("next_token"));

        // 未被 ignore 的常规 token 依然敏感
        assert!(policy.is_sensitive("user_token"));
    }

    /// Step 2 Red 测试：R3 硬键不可豁免安全护栏（恶意或错误配置无法穿透）
    #[test]
    fn test_redaction_policy_hard_keys_cannot_be_ignored() {
        // 尝试通过 ignore_keys 豁免绝对硬键
        let policy = RedactionPolicy::new(
            Vec::<&str>::new(),
            ["authorization", "cookie", "set-cookie", "x-api-key"],
        );

        // 构建期护栏守护：硬键绝对不能被装配进 ignore_keys 集合
        assert!(!policy.ignore_keys.contains("authorization"));
        assert!(!policy.ignore_keys.contains("cookie"));

        // 判定期双重护栏：即使显式声明 ignore，硬键必须坚决判定为敏感！
        assert!(policy.is_sensitive("Authorization"));
        assert!(policy.is_sensitive("authorization"));
        assert!(policy.is_sensitive("Cookie"));
        assert!(policy.is_sensitive("set-cookie"));
        assert!(policy.is_sensitive("X-Api-Key"));
    }

    /// Step 2 Red 测试：大小写与空白字符规范化
    #[test]
    fn test_case_and_whitespace_canonicalization() {
        let policy = RedactionPolicy::new(
            ["  X-Custom-Secret  ", "MY_API_KEY"],
            ["  NEXT_PAGE_CURSOR  "],
        );

        // 各种大小写与空白组合输入均能正确匹配
        assert!(policy.is_sensitive("x-custom-secret"));
        assert!(policy.is_sensitive("  X-CUSTOM-SECRET  "));
        assert!(policy.is_sensitive("my_api_key"));
        assert!(!policy.is_sensitive("next_page_cursor"));
        assert!(!policy.is_sensitive("  Next_Page_Cursor  "));
    }

    /// Step 3 Red 测试：从 TOML 字符串解析装配 RedactionPolicy
    #[test]
    fn test_redaction_policy_from_toml() {
        let toml_str = r#"
[export.redaction]
extra_keys = ["x-sentry-key", "fastly-key"]
ignore_keys = ["custom_cursor", "next_token"]
"#;
        let policy = RedactionPolicy::from_toml_str(toml_str).expect("parse toml failed");

        assert!(policy.is_sensitive("X-Sentry-Key"));
        assert!(policy.is_sensitive("fastly-key"));
        assert!(!policy.is_sensitive("custom_cursor"));
        assert!(!policy.is_sensitive("next_token"));
    }

    /// Step 3 Red 测试：自定义 Policy 驱动 URL Query 脱敏
    #[test]
    fn test_redact_url_with_custom_policy() {
        let policy = RedactionPolicy::new(["custom_sentry_key"], ["allowed_token"]);

        let url =
            "https://example.com/api?custom_sentry_key=val1&allowed_token=val2&raw_token=val3";
        let masked = policy.redact_url(url);

        // custom_sentry_key 必须掩码
        assert!(
            masked.contains("custom_sentry_key=%5BREDACTED%5D")
                || masked.contains("custom_sentry_key=[REDACTED]")
        );
        // allowed_token 被 ignore_keys 豁免，值必须原样保留！
        assert!(masked.contains("allowed_token=val2"));
        // raw_token 未豁免，必须掩码
        assert!(!masked.contains("raw_token=val3"));
    }

    /// Step 3 Red 测试：自定义 Policy 驱动 JSON Body 字段脱敏
    #[test]
    fn test_mask_body_secrets_with_custom_policy() {
        let policy = RedactionPolicy::new(["internal_device_id"], ["page_token"]);

        let body = r#"{"internal_device_id":"dev-999","page_token":"cursor-abc","user_token":"secret-xyz"}"#;
        let masked = policy.mask_body_secrets(body);

        assert!(masked.contains(r#""internal_device_id":"[REDACTED]""#));
        // page_token 豁免，原样保留
        assert!(masked.contains(r#""page_token":"cursor-abc""#));
        // user_token 未豁免，打码
        assert!(masked.contains(r#""user_token":"[REDACTED]""#));
    }

    /// Step 4 Red 测试：RedactionPolicy 结合 RequestMetadata 动态派生
    #[test]
    fn test_redaction_policy_with_request_metadata() {
        let base_policy = RedactionPolicy::builtin();
        let mut meta = crate::parser::types::RequestMetadata::default();
        meta.sensitive_keys.push("x-app-session".to_string());
        meta.no_redact_keys.push("custom_cursor".to_string());
        // 尝试通过标注豁免硬键
        meta.no_redact_keys.push("authorization".to_string());

        let derived = base_policy.with_request_metadata(&meta);

        // 1. 显式 @sensitive 生效
        assert!(derived.is_sensitive("X-App-Session"));
        // 2. 显式 @no-redact 生效
        assert!(!derived.is_sensitive("custom_cursor"));
        // 3. 硬键护栏生效，@no-redact 依然无法豁免 authorization
        assert!(derived.is_sensitive("Authorization"));
    }

    /// Step 5 Red 测试：RedactionAuditLog 审计追踪与 Footer 渲染
    #[test]
    fn test_redaction_audit_log_record_and_render_footer() {
        let policy = RedactionPolicy::builtin();
        let mut audit_log = RedactionAuditLog::new();

        // 1. 带审计记录的 Header 脱敏
        let masked_header = policy.redact_header_value_audited(
            "Authorization",
            "Bearer secret_token_12345",
            Some(&mut audit_log),
        );
        assert_eq!(masked_header, "Bearer [REDACTED]");

        // 2. 带审计记录的 URL 脱敏
        let masked_url = policy.redact_url_audited(
            "https://api.com/v1?api_key=sk-99999&page=1",
            Some(&mut audit_log),
        );
        assert!(
            masked_url.contains("api_key=%5BREDACTED%5D")
                || masked_url.contains("api_key=[REDACTED]")
        );

        // 3. 校验审计日志收集条目
        assert_eq!(audit_log.len(), 2);
        assert_eq!(audit_log.entries()[0].key, "Authorization");
        assert_eq!(audit_log.entries()[0].scheme.as_deref(), Some("Bearer"));
        assert_eq!(audit_log.entries()[0].raw_len, 25);
        assert_eq!(audit_log.entries()[0].source, RedactionSource::Header);

        assert_eq!(audit_log.entries()[1].key, "api_key");
        assert_eq!(audit_log.entries()[1].source, RedactionSource::UrlQuery);

        // 4. 渲染 Footer 到缓冲区，校验输出内容
        let mut buffer = Vec::new();
        audit_log.render_footer(&mut buffer).unwrap();
        let output = String::from_utf8(buffer).unwrap();

        assert!(output.contains("[i] Redacted 2 credential(s):"));
        assert!(output.contains("Authorization (Bearer, 25 chars) [Header]"));
        assert!(output.contains("api_key (8 chars) [URL Query]"));
    }

    #[test]
    fn test_redact_header_value_bearer() {
        assert_eq!(
            redact_header_value("Authorization", "Bearer eyJhbGciOi..."),
            "Bearer [REDACTED]"
        );
        assert_eq!(
            redact_header_value("Authorization", "bearer my_secret_token"),
            "bearer [REDACTED]"
        );
    }

    #[test]
    fn test_redact_header_value_opaque() {
        assert_eq!(
            redact_header_value("X-Api-Key", "sk-1234567890"),
            "[REDACTED]"
        );
        assert_eq!(
            redact_header_value("Cookie", "session_id=abc123xyz"),
            "[REDACTED]"
        );
        assert_eq!(
            redact_header_value("Content-Type", "application/json"),
            "application/json"
        );
    }

    #[test]
    fn test_redact_url_query() {
        let url = "https://example.com/api?api_key=secret999&page=2";
        let redacted = redact_url(url);
        assert!(
            redacted.contains("api_key=%5BREDACTED%5D") || redacted.contains("api_key=[REDACTED]")
        );
        assert!(redacted.contains("page=2"));
        assert!(!redacted.contains("secret999"));
    }

    #[test]
    fn test_redact_url_without_sensitive_query() {
        let url = "https://example.com/api?page=2&sort=asc";
        assert_eq!(redact_url(url), "https://example.com/api?page=2&sort=asc");
    }
}
