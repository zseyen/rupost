//! 掩码/脱敏原语（批三-6 拆分：自由函数族；策略实例 BUILTIN_POLICY 为
//! 便捷包装的锚点，策略判定本体见 super::policy）。

use super::policy::{RedactionPolicy, is_sensitive_key};
use regex::Regex;
use serde_json::Value;

/// JSON 原地脱敏掩码（全局默认内置策略，供 AI 归因等复用以保证规则 SSOT）
pub fn mask_json_pii_value_in_place(val: &mut serde_json::Value) {
    BUILTIN_POLICY.mask_json_pii_in_place(val);
}

pub(crate) static BUILTIN_POLICY: std::sync::LazyLock<RedactionPolicy> =
    std::sync::LazyLock::new(RedactionPolicy::builtin);

/// 对 Header 凭据值进行掩码脱敏（全局默认内置策略）
///
/// 若包含常见的认证 scheme 前缀（如 Bearer / Basic / Digest / Token），保留前缀便于识别认证模式
pub fn redact_header_value(key: &str, value: &str) -> String {
    BUILTIN_POLICY.redact_header_value(key, value)
}

/// 脱敏 URL Query 参数中的敏感键值（全局默认内置策略）
///
/// 示例：`https://api.com?api_key=secret123&page=1` -> `https://api.com/?api_key=%5BREDACTED%5D&page=1`
pub fn redact_url(raw_url: &str) -> String {
    BUILTIN_POLICY.redact_url(raw_url)
}

/// 将显式密钥值列表去空并按长度降序排列（按值掩码 SSOT）。
///
/// 必须先长后短：若短串是长串的前缀/子串，先替换短串会把长串打成
/// `[REDACTED]` 混合残片，导致长串后续无法匹配。
pub fn order_secret_values(secret_values: &[String]) -> Vec<&str> {
    let mut ordered: Vec<&str> = secret_values
        .iter()
        .map(String::as_str)
        .filter(|s| !s.is_empty())
        .collect();
    ordered.sort_by_key(|v| std::cmp::Reverse(v.len()));
    ordered
}

/// 使用已按长度降序排列的显式密钥清单，将 `text` 中出现的原始密钥替换为 `[REDACTED]`。
pub fn redact_explicit_secrets_ordered(text: &str, ordered_secrets: &[&str]) -> String {
    if ordered_secrets.is_empty() {
        return text.to_string();
    }
    let mut out = text.to_string();
    for secret in ordered_secrets {
        if !secret.is_empty() {
            out = out.replace(secret, "[REDACTED]");
        }
    }
    out
}

/// 对 `text` 中的显式密钥值列表执行按长度降序的 `[REDACTED]` 替换。
pub fn redact_explicit_secrets(text: &str, secret_values: &[String]) -> String {
    let ordered = order_secret_values(secret_values);
    redact_explicit_secrets_ordered(text, &ordered)
}

/// URL 整体替换为 `<url>` 占位符（用于 AI 去重签名：同端点不同实例同签名；
/// 原住 ai/redactor.rs（已删除的路径兼容垫片），WP 审查批四下沉归位——脱敏原语 SSOT 收口）
pub fn mask_urls(text: &str) -> String {
    static URL_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    URL_RE
        .get_or_init(|| Regex::new(r#"https?://[^\s"'<>`)]+"#).expect("valid url regex"))
        .replace_all(text, "<url>")
        .to_string()
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

/// 按字符数截断错误消息，防止 AI 后端返回的超长错误体刷屏
pub fn truncate_chars(msg: &str, max_chars: usize) -> String {
    crate::utils::text::truncate_chars(msg, max_chars, "...[truncated]")
}

/// 递归遍历 JSON Value，将命中 `is_sensitive_key` 的对象字段值替换为 `"[REDACTED]"`，
/// 对 `phone`/`mobile`/`id_card` 数值叶子及普通字符串叶子中的 PII 统一打码 `***`（结构保留）
pub fn redact_json_value(val: &mut Value) {
    mask_json_pii_value_in_place(val);
}

// ═══════════ Secrets Masker body 残余（快照 --mask-secrets，路线图条目 1） ═══════════

/// PII 掩码：中国大陆手机号 / 18 位身份证号（含末位 X/x）/ 银行卡号（13–19 位数字串）/ 邮箱 → `***`
/// （值形态掩码，与 is_sensitive_key 的键形态掩码互补；opt-in 旗标下才参与）
pub fn mask_pii(text: &str) -> String {
    static EMAIL: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static ID_CARD: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static CARD: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static PHONE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let email = EMAIL.get_or_init(|| {
        regex::Regex::new(r#"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}"#).expect("email regex")
    });
    // regex crate 无 lookaround——用捕获组锚定非数字边界（${1}/${2} 原样保留）
    // 18 位身份证号（17 位数字 + X/x）必须先于 CARD 匹配，防止末位 X 残余为 "***X"
    let id_card = ID_CARD.get_or_init(|| {
        regex::Regex::new(r"(^|[^0-9])[0-9]{17}[Xx]([^0-9A-Za-z]|$)").expect("id card regex")
    });
    let card = CARD.get_or_init(|| {
        // 13–19 位数字形态风险由 opt-in 旗标承担（PRD D1）
        regex::Regex::new(r"(^|[^0-9])[0-9]{13,19}([^0-9]|$)").expect("card regex")
    });
    let phone = PHONE.get_or_init(|| {
        regex::Regex::new(r"(^|[^0-9])1[3-9][0-9]{9}([^0-9]|$)").expect("phone regex")
    });
    let s = email.replace_all(text, "***").into_owned();
    let s = id_card.replace_all(&s, "${1}***${2}").into_owned();
    let s = card.replace_all(&s, "${1}***${2}").into_owned();
    phone.replace_all(&s, "${1}***${2}").into_owned()
}

/// 快照 body 掩码（全局默认内置策略）
pub fn mask_body_secrets(body: &str) -> String {
    RedactionPolicy::builtin().mask_body_secrets(body)
}

/// 自由文本 URL 掩码（全局默认内置策略）
pub fn redact_free_text(text: &str) -> String {
    RedactionPolicy::builtin().redact_free_text(text)
}

/// 非 JSON 文本的内联键值凭据兜底掩码：`token=xxx` / `api_key: xxx` / `x-sign: xxx` / `cookie: xxx` / 孤立 `Bearer <token>` / `Basic <base64>` 形态
/// （键名保留、值替换为 [REDACTED]；值长度 ≥4 才掩，避免误伤短 token 字样）
pub fn mask_inline_secrets(text: &str) -> String {
    static INLINE_SECRET: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static BARE_AUTH: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = INLINE_SECRET.get_or_init(|| {
        regex::Regex::new(
            r#"(?i)(^|[^a-zA-Z0-9])(token|api[_-]?key|secret|password|passwd|authorization|x-sign|sign|set-cookie|cookie)\s*[=:]\s*(?:(?:bearer|basic|digest|token)\s+)?[^\s"'<>&,;}{]{4,}"#,
        )
        .expect("inline secret regex")
    });
    let re_auth = BARE_AUTH.get_or_init(|| {
        regex::Regex::new(r#"(?i)\b(bearer|basic)\s+[A-Za-z0-9._~+/-]{10,}={0,2}"#)
            .expect("bare auth regex")
    });
    let step1 = re.replace_all(text, "${1}${2}=[REDACTED]");
    re_auth.replace_all(&step1, "${1} [REDACTED]").into_owned()
}

/// 默认响应体片段字节上限（512 Bytes）
pub const DEFAULT_SNIPPET_MAX_BYTES: usize = 512;

/// 判断断言 raw 引用的字段是否命中敏感键（如 `body.access_token`、`headers.Authorization`）
pub fn assertion_targets_sensitive_key(raw: &str) -> bool {
    raw.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .any(is_sensitive_key)
}

/// 在 UTF-8 字符边界安全截断文本（按字节上限），超出时追加 `...[truncated]`。
pub fn truncate_utf8_safe(text: &str, max_bytes: usize) -> String {
    crate::utils::text::truncate_utf8_bytes(text, max_bytes, "...[truncated]")
}

/// 对响应体进行结构化脱敏并安全截断至 `max_bytes`
pub fn redact_and_truncate_body(body: &str, max_bytes: usize) -> String {
    // 盲扫 #5：掩码逻辑委托 `RedactionPolicy::mask_body_secrets` 单一实现
    // （此前两份逐行同构的 JSON/非 JSON 分支靠 parity 测试锁同步）
    let masked = RedactionPolicy::builtin().mask_body_secrets(body);

    truncate_utf8_safe(&masked, max_bytes)
}

#[cfg(test)]
mod mask_tests {
    use super::*;

    /// Secrets Masker 决策表：PII 三形态 + 键形态 + 复合串不误伤
    #[test]
    fn test_mask_pii_and_body_secrets_decision_table() {
        // 手机号（含边界非数字保护）
        assert_eq!(mask_pii("联系 13812345678 处理"), "联系 *** 处理");
        assert_eq!(
            mask_pii("id=113812345678"),
            "id=113812345678",
            "长串内不误切"
        );
        // 银行卡号（13–19 位）
        assert_eq!(mask_pii("card=6222021234567890123"), "card=***");
        // 邮箱
        assert_eq!(mask_pii("mail lee@example.com ok"), "mail *** ok");
        // 敏感键 JSON：值整体 [REDACTED]；普通键下的 PII 叶子也打码
        let out = mask_body_secrets(r#"{"password":"p@ss1234","phone":"13812345678","memo":"ok"}"#);
        assert!(out.contains("[REDACTED]"), "sensitive key masked: {out}");
        assert!(out.contains("***"), "PII leaf masked: {out}");
        assert!(
            out.contains("\"memo\":\"ok\""),
            "plain leaf preserved: {out}"
        );
        // 非 JSON：内联凭据 + PII
        let out = mask_body_secrets("token=abcdef123456 call 13812345678");
        assert!(
            out.contains("token=[REDACTED]"),
            "inline credential masked: {out}"
        );
        assert!(out.contains("***"), "pii masked in text: {out}");
        // 空体
        assert_eq!(mask_body_secrets(""), "");
    }

    #[test]
    fn test_token_aware_sensitive_key_and_no_false_positives_table() {
        let cases = [
            // 必须脱敏的新增/既有敏感键
            ("X-Sign", true),
            ("x-sign", true),
            ("X-Signature", true),
            ("x-signature", true),
            ("Authorization", true),
            ("Authorization-Ext", true),
            ("authorization_custom", true),
            ("sign", true),
            ("sig", true),
            ("signature", true),
            ("key", true),
            ("passwd", true),
            ("user_passwd", true),
            ("api_key", true),
            ("x-api-key", true),
            ("Cookie", true),
            ("Set-Cookie", true),
            // 绝不误报的普通键
            ("design", false),
            ("assign", false),
            ("signal", false),
            ("keyboard", false),
            ("primary_key", false),
            ("next_token", false),
            ("page_token", false),
            ("Content-Type", false),
            ("page", false),
        ];
        for (key, expected) in cases {
            assert_eq!(
                is_sensitive_key(key),
                expected,
                "is_sensitive_key({key:?}) should be {expected}"
            );
        }

        // Authorization* 不可被 ignore_keys 豁免；非硬键（如 fastly-key / x-api-key-safe）仍可豁免
        let policy = RedactionPolicy::new(
            Vec::<&str>::new(),
            ["authorization-ext", "fastly-key", "x-api-key-safe"],
        );
        assert!(policy.is_sensitive("Authorization-Ext"));
        assert!(!policy.is_sensitive("fastly-key"));
        assert!(!policy.is_sensitive("x-api-key-safe"));
    }

    #[test]
    fn test_headers_and_url_query_redaction_table() {
        let header_cases = [
            ("Authorization", "Bearer jwt-123", "Bearer [REDACTED]"),
            ("Authorization-Ext", "Bearer ext-123", "Bearer [REDACTED]"),
            ("Authorization-Ext", "opaque-ext", "[REDACTED]"),
            ("Cookie", "sid=abc123", "[REDACTED]"),
            ("Set-Cookie", "sid=abc123; HttpOnly", "[REDACTED]"),
            ("X-Sign", "sig-deadbeef", "[REDACTED]"),
            ("X-Signature", "hmac-sha256=deadbeef", "[REDACTED]"),
            ("Content-Type", "application/json", "application/json"),
        ];
        for (k, v, expected) in header_cases {
            assert_eq!(
                redact_header_value(k, v),
                expected,
                "header {k} redaction mismatch"
            );
        }

        let url = "https://api.example.com/v1/items?token=t1&access_token=at2&sign=s3&sig=s4&key=k5&secret=sec6&page=2&next_token=nt7&design=modern&signal=ok";
        let redacted = redact_url(url);
        for leaked in ["t1", "at2", "s3", "s4", "k5", "sec6"] {
            assert!(
                !redacted.contains(&format!("={leaked}")),
                "sensitive query value {leaked} leaked in {redacted}"
            );
        }
        for preserved in ["page=2", "next_token=nt7", "design=modern", "signal=ok"] {
            assert!(
                redacted.contains(preserved),
                "non-sensitive query {preserved} dropped in {redacted}"
            );
        }
    }

    #[test]
    fn test_mask_pii_id_card_and_variants_table() {
        let cases = [
            ("11010119900101123X", "***"),
            ("11010119900101123x", "***"),
            ("id_card=11010119900101123X, ok", "id_card=***, ok"),
            ("110101199001011234", "***"),
            ("13812345678", "***"),
            ("card=6222021234567890123", "card=***"),
            ("mail lee@example.com ok", "mail *** ok"),
            ("id=113812345678", "id=113812345678"),
            ("12345678901234567xyz", "***xyz"),
        ];
        for (input, expected) in cases {
            assert_eq!(mask_pii(input), expected, "mask_pii({input:?}) mismatch");
        }
    }

    #[test]
    fn test_mask_body_secrets_json_string_number_and_nested_table() {
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
                "mobile": "13612345678",
                "id_card": "11010119900101123X",
                "passwd": "nested_pw",
                "primary_key": 42,
                "timestamp_ms": 1731396798000,
                "design": "minimal",
                "assign": "alice",
                "signal": "green",
                "keyboard": "hhkb"
            },
            "items": [{"key": "k-secret", "page": 1}]
        }"#;
        let masked = mask_body_secrets(raw);
        let parsed: serde_json::Value = serde_json::from_str(&masked).unwrap();

        assert_eq!(parsed["password"], "[REDACTED]");
        assert_eq!(parsed["passwd"], "[REDACTED]");
        assert_eq!(parsed["secret"], "[REDACTED]");
        assert_eq!(parsed["token"], "[REDACTED]");
        assert_eq!(parsed["access_token"], "[REDACTED]");
        assert_eq!(parsed["phone"], "***");
        assert_eq!(parsed["mobile"], "***");
        assert_eq!(parsed["id_card"], "***");
        assert_eq!(parsed["nested"]["phone"], "***");
        assert_eq!(parsed["nested"]["mobile"], "***");
        assert_eq!(parsed["nested"]["id_card"], "***");
        assert_eq!(parsed["nested"]["passwd"], "[REDACTED]");
        assert_eq!(parsed["nested"]["primary_key"], 42);
        assert_eq!(parsed["nested"]["timestamp_ms"], 1731396798000_i64);
        assert_eq!(parsed["nested"]["design"], "minimal");
        assert_eq!(parsed["nested"]["assign"], "alice");
        assert_eq!(parsed["nested"]["signal"], "green");
        assert_eq!(parsed["nested"]["keyboard"], "hhkb");
        assert_eq!(parsed["items"][0]["key"], "[REDACTED]");
        assert_eq!(parsed["items"][0]["page"], 1);
    }

    #[test]
    fn test_mask_inline_secrets_expanded_table() {
        let cases = [
            ("X-Sign: deadbeef", "X-Sign=[REDACTED]"),
            ("sign=deadbeef&page=1", "sign=[REDACTED]&page=1"),
            ("Cookie: sid=abc12345", "Cookie=[REDACTED]"),
            (
                "Set-Cookie: sid=abc12345; HttpOnly",
                "Set-Cookie=[REDACTED]; HttpOnly",
            ),
            ("passwd=hunter2", "passwd=[REDACTED]"),
            (
                "design=abcdef assign=abcdef signal=abcdef",
                "design=abcdef assign=abcdef signal=abcdef",
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(
                mask_inline_secrets(input),
                expected,
                "mask_inline_secrets({input:?}) mismatch"
            );
        }
    }

    #[test]
    fn test_redaction_policy_hard_key_guard_emits_to_buffered_sink() {
        use crate::diagnostics::{BufferedWarningSink, WarningSeverity};
        let sink = BufferedWarningSink::new();
        let policy =
            RedactionPolicy::new_with_sink(std::iter::empty::<&str>(), ["authorization"], &sink);
        assert!(policy.is_sensitive("Authorization"));

        let mut meta = crate::parser::types::RequestMetadata::default();
        meta.no_redact_keys.push("x-api-key".to_string());
        let derived = policy.with_request_metadata_with_sink(&meta, &sink);
        assert!(derived.is_sensitive("x-api-key"));

        let recs = sink.records();
        assert_eq!(recs.len(), 2);
        assert!(recs.iter().all(|r| r.severity == WarningSeverity::Security));
    }
}

// —— body 掩码族测试（原 ai/redactor.rs 垫片的 15 项独有覆盖，垫片已删、
// 测试并入本 SSOT 文件；命名按主题而非出生地——审查盲扫 #6）——
#[cfg(test)]
mod body_masking_tests {
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
