//! 敏感凭据脱敏工具

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

use std::collections::HashSet;

#[inline]
fn is_hard_sensitive_key(key_str: &str) -> bool {
    EXACT_KEYS.binary_search(&key_str).is_ok() || key_str.starts_with("authorization")
}

#[inline]
fn matches_fuzzy_sensitive_key(key_str: &str) -> bool {
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

/// 判断 Header 键名是否属于敏感信息（全局内置默认判定）
#[inline]
pub fn is_sensitive_key(key: &str) -> bool {
    BUILTIN_POLICY.is_sensitive(key)
}

/// 脱敏策略控制中心
#[derive(Debug, Clone)]
pub struct RedactionPolicy {
    /// 项目自定义新增的敏感键（强制判定为敏感）
    extra_keys: HashSet<String>,
    /// 用户与内置的豁免键（豁免 SUBSTRINGS 模糊匹配）
    ignore_keys: HashSet<String>,
}

impl Default for RedactionPolicy {
    fn default() -> Self {
        Self::builtin()
    }
}

#[derive(Debug, serde::Deserialize, Default)]
struct TomlRootForRedaction {
    #[serde(default)]
    export: Option<TomlExportForRedaction>,
}

#[derive(Debug, serde::Deserialize, Default)]
struct TomlExportForRedaction {
    #[serde(default)]
    redaction: Option<TomlRedactionConfig>,
}

#[derive(Debug, serde::Deserialize, Default)]
struct TomlRedactionConfig {
    #[serde(default)]
    extra_keys: Vec<String>,
    #[serde(default)]
    ignore_keys: Vec<String>,
}

/// 脱敏凭据来源
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedactionSource {
    Header,
    UrlQuery,
    UrlFragment,
    BodyJson,
}

/// 单条脱敏凭据审计条目
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedactionAuditEntry {
    pub key: String,
    pub scheme: Option<String>,
    pub raw_len: usize,
    pub source: RedactionSource,
}

/// 脱敏审计日志收集器
#[derive(Debug, Clone, Default)]
pub struct RedactionAuditLog {
    entries: Vec<RedactionAuditEntry>,
}

impl RedactionAuditLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(
        &mut self,
        key: &str,
        scheme: Option<&str>,
        raw_len: usize,
        source: RedactionSource,
    ) {
        self.entries.push(RedactionAuditEntry {
            key: key.to_string(),
            scheme: scheme.map(|s| s.to_string()),
            raw_len,
            source,
        });
    }

    pub fn entries(&self) -> &[RedactionAuditEntry] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 渲染脱敏清单 Footer（必须输出至 stderr，确保 stdout 命令纯净）
    pub fn render_footer<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        if self.entries.is_empty() {
            return Ok(());
        }

        writeln!(
            writer,
            "\n[i] Redacted {} credential(s):",
            self.entries.len()
        )?;
        for entry in &self.entries {
            let scheme_desc = match &entry.scheme {
                Some(s) => format!("{}, ", s),
                None => String::new(),
            };
            let source_desc = match entry.source {
                RedactionSource::Header => "Header",
                RedactionSource::UrlQuery => "URL Query",
                RedactionSource::UrlFragment => "URL Fragment",
                RedactionSource::BodyJson => "Body JSON",
            };
            writeln!(
                writer,
                "  - {} ({}{} chars) [{}]",
                entry.key, scheme_desc, entry.raw_len, source_desc
            )?;
        }
        Ok(())
    }
}

impl RedactionPolicy {
    /// 从 TOML 配置字符串解析装配 [export.redaction] 规则
    pub fn from_toml_str(toml_str: &str) -> Result<Self, String> {
        let root: TomlRootForRedaction = toml::from_str(toml_str)
            .map_err(|e| format!("Failed to parse [export.redaction] config: {e}"))?;

        if let Some(export) = root.export
            && let Some(redaction) = export.redaction
        {
            Ok(Self::new(redaction.extra_keys, redaction.ignore_keys))
        } else {
            Ok(Self::builtin())
        }
    }

    /// 获取内核内置默认策略
    pub fn builtin() -> Self {
        Self {
            extra_keys: HashSet::new(),
            ignore_keys: DEFAULT_IGNORE_KEYS.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// 构建包含外部配置与活文档标注的定制策略
    pub fn new(
        extra_keys: impl IntoIterator<Item = impl AsRef<str>>,
        ignore_keys: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Self {
        let mut policy = Self::builtin();

        for key in extra_keys {
            let normalized = key.as_ref().trim().to_ascii_lowercase();
            if !normalized.is_empty() {
                policy.extra_keys.insert(normalized);
            }
        }

        for key in ignore_keys {
            let normalized = key.as_ref().trim().to_ascii_lowercase();
            if normalized.is_empty() {
                continue;
            }
            // R3 安全护栏：严禁豁免硬键！
            if is_hard_sensitive_key(normalized.as_str()) {
                eprintln!(
                    "[warn] 安全护栏阻断：硬键 '{normalized}' 属于核心凭据，不可被 ignore 豁免！"
                );
                continue;
            }
            policy.ignore_keys.insert(normalized);
        }

        policy
    }

    /// 结合请求级元数据 (@sensitive / @no-redact) 派生新策略（活文档就地标注优先于全局配置）
    pub fn with_request_metadata(&self, meta: &crate::parser::types::RequestMetadata) -> Self {
        if meta.sensitive_keys.is_empty() && meta.no_redact_keys.is_empty() {
            return self.clone();
        }
        let mut derived = self.clone();
        for k in &meta.sensitive_keys {
            let normalized = k.trim().to_ascii_lowercase();
            if !normalized.is_empty() {
                derived.extra_keys.insert(normalized);
            }
        }
        for k in &meta.no_redact_keys {
            let normalized = k.trim().to_ascii_lowercase();
            if normalized.is_empty() {
                continue;
            }
            if is_hard_sensitive_key(normalized.as_str()) {
                eprintln!(
                    "[warn] 安全护栏阻断：文件内标注 @no-redact 无法豁免硬键 '{normalized}'！"
                );
                continue;
            }
            derived.ignore_keys.insert(normalized);
        }
        derived
    }

    /// 核心判定函数：判断键名是否属于敏感信息
    #[inline]
    pub fn is_sensitive(&self, key: &str) -> bool {
        let lower = key.trim().to_ascii_lowercase();
        let key_str = lower.as_str();

        // 1. 用户显式标注 / 配置的敏感键优先
        if self.extra_keys.contains(key_str) {
            return true;
        }

        // 2. 内置硬键（含 Authorization*）不可穿透
        if is_hard_sensitive_key(key_str) {
            return true;
        }

        // 3. 豁免检查（只作用于模糊/词根匹配层）
        if self.ignore_keys.contains(key_str) {
            return false;
        }

        // 4. 内置模糊词族子串与词根匹配
        matches_fuzzy_sensitive_key(key_str)
    }

    /// 对 Header 凭据值进行掩码脱敏（支持可选审计记录）
    pub fn redact_header_value_audited(
        &self,
        key: &str,
        value: &str,
        mut audit_log: Option<&mut RedactionAuditLog>,
    ) -> String {
        if !self.is_sensitive(key) {
            return value.to_string();
        }
        let (masked, scheme) = if let Some((scheme, rest)) = value.split_once(' ')
            && !rest.trim().is_empty()
            && matches!(
                scheme.to_ascii_uppercase().as_str(),
                "BEARER" | "BASIC" | "DIGEST" | "TOKEN"
            ) {
            (format!("{scheme} [REDACTED]"), Some(scheme))
        } else {
            ("[REDACTED]".to_string(), None)
        };

        if let Some(ref mut log) = audit_log {
            log.record(key, scheme, value.len(), RedactionSource::Header);
        }

        masked
    }

    /// 对 Header 凭据值进行掩码脱敏
    pub fn redact_header_value(&self, key: &str, value: &str) -> String {
        self.redact_header_value_audited(key, value, None)
    }

    /// 对形如 `key=value&k2=v2` 的序列进行脱敏，复用 policy.is_sensitive
    /// 返回 `(脱敏后的序列字符串, 是否发生改变)`
    pub fn redact_key_value_pairs(
        &self,
        pairs: impl IntoIterator<Item = (impl AsRef<str>, impl AsRef<str>)>,
        source: RedactionSource,
        mut audit_log: Option<&mut RedactionAuditLog>,
    ) -> (String, bool) {
        let mut serializer = url::form_urlencoded::Serializer::new(String::new());
        let mut changed = false;

        for (k, v) in pairs {
            let k_ref = k.as_ref();
            let v_ref = v.as_ref();
            if self.is_sensitive(k_ref) {
                if let Some(ref mut log) = audit_log {
                    log.record(k_ref, None, v_ref.len(), source);
                }
                serializer.append_pair(k_ref, "[REDACTED]");
                changed = true;
            } else {
                serializer.append_pair(k_ref, v_ref);
            }
        }

        (serializer.finish(), changed)
    }

    /// 脱敏 URL Query 与 Fragment 参数中的敏感键值（支持可选审计记录）
    pub fn redact_url_audited(
        &self,
        raw_url: &str,
        mut audit_log: Option<&mut RedactionAuditLog>,
    ) -> String {
        let Ok(mut parsed) = url::Url::parse(raw_url) else {
            return raw_url.to_string();
        };
        let mut changed = false;

        // userinfo 密码段（https://user:pass@host）
        if let Some(pass) = parsed.password() {
            let pass_len = pass.len();
            let _ = parsed.set_password(Some("[REDACTED]"));
            changed = true;
            if let Some(ref mut log) = audit_log {
                log.record("password", None, pass_len, RedactionSource::UrlQuery);
            }
        }

        // fragment 凭据（OAuth implicit 流：#access_token=...，SPA 路由：#/login?token=...）
        if let Some(frag) = parsed.fragment() {
            if let Some((path, query_str)) = frag.split_once('?') {
                // SPA 路由形态：#/path?token=secret123&env=prod
                let pairs: Vec<(String, String)> =
                    url::form_urlencoded::parse(query_str.as_bytes())
                        .map(|(k, v)| (k.into_owned(), v.into_owned()))
                        .collect();
                if pairs.iter().any(|(k, _)| self.is_sensitive(k)) {
                    let (redacted_query, was_changed) = self.redact_key_value_pairs(
                        pairs,
                        RedactionSource::UrlFragment,
                        audit_log.as_deref_mut(),
                    );
                    if was_changed {
                        let new_frag = format!("{path}?{redacted_query}");
                        parsed.set_fragment(Some(&new_frag));
                        changed = true;
                    }
                }
            } else if frag.contains('=') {
                // OAuth implicit 流形态：#access_token=secret123&state=xyz
                let pairs: Vec<(String, String)> = url::form_urlencoded::parse(frag.as_bytes())
                    .map(|(k, v)| (k.into_owned(), v.into_owned()))
                    .collect();
                if pairs.iter().any(|(k, _)| self.is_sensitive(k)) {
                    let (redacted_frag, was_changed) = self.redact_key_value_pairs(
                        pairs,
                        RedactionSource::UrlFragment,
                        audit_log.as_deref_mut(),
                    );
                    if was_changed {
                        parsed.set_fragment(Some(&redacted_frag));
                        changed = true;
                    }
                }
            }
            // 纯锚点形态（无 '?' 且无 '='），如 #keyboard-shortcuts、#accessibility、#section-2
            // 纯锚点不包含键值参数，100% 保持原样，彻底避免误杀！
        }

        // query 敏感键值
        let pairs: Vec<(String, String)> = parsed
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        if pairs.iter().any(|(k, _)| self.is_sensitive(k)) {
            let (new_query, was_changed) =
                self.redact_key_value_pairs(pairs, RedactionSource::UrlQuery, audit_log);
            if was_changed {
                parsed.set_query(Some(&new_query));
                changed = true;
            }
        }

        if changed {
            parsed.into()
        } else {
            raw_url.to_string()
        }
    }

    /// 脱敏 URL Query 参数中的敏感键值
    pub fn redact_url(&self, raw_url: &str) -> String {
        self.redact_url_audited(raw_url, None)
    }

    /// 自由文本 URL 掩码
    pub fn redact_free_text(&self, text: &str) -> String {
        static URL_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let re = URL_RE
            .get_or_init(|| regex::Regex::new(r#"https?://[^\s"'<>`)]+"#).expect("url regex"));
        re.replace_all(text, |caps: &regex::Captures| self.redact_url(&caps[0]))
            .into_owned()
    }

    /// 快照 body 掩码：支持注入定制策略，豁免无需打码的字段
    pub fn mask_body_secrets(&self, body: &str) -> String {
        let trimmed = body.trim();
        if trimmed.is_empty() {
            return String::new();
        }
        if (trimmed.starts_with('{') || trimmed.starts_with('['))
            && let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed)
        {
            let out = self.mask_json_pii(&val);
            serde_json::to_string(&out).unwrap_or_else(|_| trimmed.to_string())
        } else {
            mask_pii(&mask_inline_secrets(&self.redact_free_text(trimmed)))
        }
    }

    /// JSON 掩码变体（原地改写）：敏感键 `[REDACTED]`，PII 数值键（phone/mobile/id_card）`***`，字符串叶子上的 PII 也打码
    pub fn mask_json_pii_in_place(&self, val: &mut serde_json::Value) {
        match val {
            serde_json::Value::Object(map) => {
                for (k, v) in map.iter_mut() {
                    if self.is_sensitive(k) {
                        *v = serde_json::Value::String("[REDACTED]".to_string());
                    } else if is_pii_key(k) && v.is_number() {
                        *v = serde_json::Value::String("***".to_string());
                    } else {
                        self.mask_json_pii_in_place(v);
                    }
                }
            }
            serde_json::Value::Array(items) => {
                for item in items.iter_mut() {
                    self.mask_json_pii_in_place(item);
                }
            }
            serde_json::Value::String(s) => {
                *s = mask_pii(s);
            }
            _ => {}
        }
    }

    /// JSON 掩码变体：敏感键 `[REDACTED]` 之外，PII 数值键与普通字符串叶子上的 PII 也打码
    pub fn mask_json_pii(&self, val: &serde_json::Value) -> serde_json::Value {
        let mut cloned = val.clone();
        self.mask_json_pii_in_place(&mut cloned);
        cloned
    }
}

/// JSON 原地脱敏掩码（全局默认内置策略，供 `ai::redactor` 等复用以保证规则 SSOT）
pub fn mask_json_pii_value_in_place(val: &mut serde_json::Value) {
    BUILTIN_POLICY.mask_json_pii_in_place(val);
}

static BUILTIN_POLICY: std::sync::LazyLock<RedactionPolicy> =
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
}
