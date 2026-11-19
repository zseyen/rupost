//! 脱敏策略控制中心（批三-6 拆分：策略判定与派生；词表见 super、掩码原语见 primitives）。

use std::collections::HashSet;

use super::is_pii_key;
use super::primitives::{BUILTIN_POLICY, mask_inline_secrets, mask_pii};
use super::{DEFAULT_IGNORE_KEYS, is_hard_sensitive_key, matches_fuzzy_sensitive_key};
use crate::security::redact_audit::{RedactionAuditLog, RedactionSource};

/// 判断 Header 键名是否属于敏感信息（全局内置默认判定）
#[inline]
pub fn is_sensitive_key(key: &str) -> bool {
    BUILTIN_POLICY.is_sensitive(key)
}

/// 脱敏策略控制中心
#[derive(Debug, Clone)]
pub struct RedactionPolicy {
    /// 项目自定义新增的敏感键（强制判定为敏感）
    pub(crate) extra_keys: HashSet<String>,
    /// 用户与内置的豁免键（审查批二 C-d 裁决：子串包含豁免——键名精确等于
    /// 豁免键、或包含豁免键作为子串，均豁免模糊匹配层；后缀变体如
    /// `next_page_token_v2` 可被 `next_page_token` 豁免。硬键护栏前置不可豁免。
    /// 注：豁免键自身也是子串语义——`ignore=["token"]` 会豁免全部含 token 的键，
    /// 用户自行承担粒度）
    pub(crate) ignore_keys: HashSet<String>,
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
        Self::new_with_sink(
            extra_keys,
            ignore_keys,
            &crate::diagnostics::StderrWarningSink,
        )
    }

    /// 构建包含外部配置与活文档标注的定制策略（通过 `WarningSink` 传递护栏阻断告警）
    pub fn new_with_sink(
        extra_keys: impl IntoIterator<Item = impl AsRef<str>>,
        ignore_keys: impl IntoIterator<Item = impl AsRef<str>>,
        sink: &dyn crate::diagnostics::WarningSink,
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
                sink.emit(
                    crate::diagnostics::WarningSeverity::Security,
                    "security::redact",
                    &format!(
                        "安全护栏阻断：硬键 '{normalized}' 属于核心凭据，不可被 ignore 豁免！"
                    ),
                );
                continue;
            }
            policy.ignore_keys.insert(normalized);
        }

        policy
    }

    /// 结合请求级元数据 (@sensitive / @no-redact) 派生新策略（活文档就地标注优先于全局配置）
    pub fn with_request_metadata(&self, meta: &crate::parser::types::RequestMetadata) -> Self {
        self.with_request_metadata_with_sink(meta, &crate::diagnostics::StderrWarningSink)
    }

    /// 结合请求级元数据派生新策略（通过 `WarningSink` 传递护栏阻断告警）
    pub fn with_request_metadata_with_sink(
        &self,
        meta: &crate::parser::types::RequestMetadata,
        sink: &dyn crate::diagnostics::WarningSink,
    ) -> Self {
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
                sink.emit(
                    crate::diagnostics::WarningSeverity::Security,
                    "security::redact",
                    &format!("安全护栏阻断：文件内标注 @no-redact 无法豁免硬键 '{normalized}'！"),
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

        // 3. 豁免检查（只作用于模糊匹配层；精确命中或包含豁免键为子串均豁免）
        if self.ignore_keys.contains(key_str)
            || self
                .ignore_keys
                .iter()
                .any(|ig| !ig.is_empty() && key_str.contains(ig.as_str()))
        {
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
