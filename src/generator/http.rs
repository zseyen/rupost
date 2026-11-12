use crate::Result;
use crate::history::model::HistoryEntry;
use crate::security::redact::is_sensitive_key;
use colored::Colorize;
use std::collections::{BTreeSet, HashMap};

fn is_key_sensitive(key: &str, policy: Option<&crate::security::redact::RedactionPolicy>) -> bool {
    if let Some(p) = policy {
        p.is_sensitive(key)
    } else {
        is_sensitive_key(key)
    }
}

pub struct HttpGenerator;

impl HttpGenerator {
    /// Convert a list of history entries to .http file content
    pub fn generate(entries: &[HistoryEntry]) -> Result<String> {
        Ok(Self::generate_with_variables(entries, false)?.0)
    }

    /// Convert history entries to .http content, plus the sorted names of all
    /// variables referenced by templated credentials.
    ///
    /// 自动从配置中加载脱敏策略 [export.redaction]，未配置时回退到内置默认策略。
    pub fn generate_with_variables(
        entries: &[HistoryEntry],
        templatize_host: bool,
    ) -> Result<(String, Vec<String>)> {
        Self::generate_with_variables_ex(entries, templatize_host, false)
    }

    /// E-3（免重填）：`keep_credentials = true` 保留明文凭据（T2 溯源不命中时免手工回填）
    pub fn generate_with_variables_ex(
        entries: &[HistoryEntry],
        templatize_host: bool,
        keep_credentials: bool,
    ) -> Result<(String, Vec<String>)> {
        let policy = crate::variable::ConfigLoader::find_and_load_redaction_policy();
        Self::generate_with_policy(entries, templatize_host, Some(&policy), keep_credentials)
    }

    /// 接受显式脱敏策略的生成入口。
    /// E-3（免重填）：`keep_credentials = true` 时保留明文凭据（T2 溯源不命中场景
    /// 免手工回填 token）——生成文件自带 0600 建议警告，调用方须妥善保管。
    pub fn generate_with_policy(
        entries: &[HistoryEntry],
        templatize_host: bool,
        policy: Option<&crate::security::redact::RedactionPolicy>,
        keep_credentials: bool,
    ) -> Result<(String, Vec<String>)> {
        let mut output = String::new();
        let mut variables = BTreeSet::new();
        let mut used_names: HashMap<String, usize> = HashMap::new();

        // T2：链式值溯源（时间序即拓扑序；上游产出 → 下游 {{capture}} 重建数据流）
        let trace_plan = super::trace::build_trace_plan_with_policy(entries, policy);
        let has_trace =
            !trace_plan.links.is_empty() || !trace_plan.upstream_cookie_names.is_empty();

        for (i, entry) in entries.iter().enumerate() {
            if i > 0 {
                output.push_str("\n\n");
            }
            let captures: Vec<&super::trace::TracedLink> = trace_plan
                .links
                .iter()
                .filter(|l| l.upstream_idx == i)
                .collect();
            output.push_str(&Self::format_entry(
                entry,
                templatize_host,
                &mut variables,
                &mut used_names,
                if captures.is_empty() {
                    None
                } else {
                    Some(captures.as_slice())
                },
                if has_trace { Some(&trace_plan) } else { None },
                policy,
                keep_credentials,
            ));
        }

        if !entries.is_empty() {
            output.push('\n');
        }

        let names: Vec<String> = variables.into_iter().collect();
        if !names.is_empty() {
            output = format!(
                "# Credentials are templated as variables - provide values via --var, .env or rupost.toml\n# Variables: {}\n\n",
                names.join(", ")
            ) + &output;
        }

        Ok((output, names))
    }

    #[allow(clippy::too_many_arguments)] // E-3：keep_credentials 沿既有链路透传，重构签名超出本批范围
    fn format_entry(
        entry: &HistoryEntry,
        templatize_host: bool,
        variables: &mut BTreeSet<String>,
        used_names: &mut HashMap<String, usize>,
        captures: Option<&[&super::trace::TracedLink]>,
        traced: Option<&super::trace::TracePlan>,
        policy: Option<&crate::security::redact::RedactionPolicy>,
        // E-3：保留明文凭据豁免（--keep-credentials 透传）
        keep_credentials: bool,
    ) -> String {
        let mut block = String::new();

        // 1. Semantic title / name（method + path，同文件冲突时追加序号）
        let (title, name) = Self::semantic_title_and_name(entry, used_names);
        block.push_str(&format!("### {title}\n"));
        block.push_str(&format!("# @name {name}\n"));

        // 1.2 T2：本条上游产出的 capture 指令（现行 parser 语法 body.<path>/header.<name>）
        if let Some(links) = captures {
            for l in links {
                block.push_str(&format!(
                    "# @capture {} from {}\n",
                    l.capture_name, l.capture_source
                ));
            }
        }

        // 1.5 Timeout（快照记录了 @timeout 时还原该指令）
        if let Some(t) = entry.request.timeout {
            block.push_str(&format!("# @timeout {}ms\n", t.as_millis()));
        }

        // 2. Request Line（可选 origin → {{base_url}}；query 敏感键凭据/T2 溯源/T1 形状）
        let rendered_url = if templatize_host {
            Self::variabilize_url(
                &Self::templatize_host_in_url(&entry.request.url, variables),
                variables,
                traced,
                policy,
            )
        } else {
            Self::variabilize_url(&entry.request.url, variables, traced, policy)
        };
        block.push_str(&format!("{} {}\n", entry.request.method, rendered_url));

        // 3. Headers
        for (key, value) in &entry.request.headers {
            let key_str = key.as_str();
            // Skip common auto-headers that shouldn't be hardcoded in tests
            if Self::should_skip_header(key_str) {
                continue;
            }
            // lossy 读回：非 ASCII（obs-text）值无损渲染——旧 to_str() 使其被静默跳过不渲染
            let val_str = String::from_utf8_lossy(value.as_bytes()).into_owned();
            // T2 Cookie 链删头：Cookie 头命中上游 Set-Cookie 名 → 省略（AutoPersist 靠罐接续）
            if key_str.eq_ignore_ascii_case("cookie")
                && let Some(plan) = traced
                && plan
                    .upstream_cookie_names
                    .iter()
                    .any(|n| val_str.contains(n.as_str()))
            {
                continue;
            }
            // T2 最高优先（PRD 管线序）：溯源命中值豁免凭据模板化——
            // `Bearer <token>` 还原为 `Bearer {{token}}`（保留 scheme 前缀）
            let traced_hit = traced.and_then(|plan| {
                let (prefix, core) = match val_str.as_str().split_once(' ') {
                    Some((p, r)) => (Some(p), r.trim()),
                    None => (None, val_str.as_str()),
                };
                plan.links
                    .iter()
                    .find(|l| l.value == core)
                    .map(|l| match prefix {
                        Some(p) => format!("{p} {{{{{}}}}}", l.capture_name),
                        None => format!("{{{{{}}}}}", l.capture_name),
                    })
            });
            if let Some(rendered) = traced_hit {
                block.push_str(&format!("{key_str}: {rendered}\n"));
                continue;
            }
            // 凭据模板化：生成的测试文件严禁落盘明文凭证
            // （E-3：--keep-credentials 显式豁免——保留字面值免手工回填）
            if is_key_sensitive(key_str, policy) && !keep_credentials {
                let var = Self::var_name_from_key(key_str);
                variables.insert(var.clone());
                match Self::split_auth_scheme(&val_str) {
                    Some(scheme) => {
                        block.push_str(&format!("{key_str}: {scheme} {{{{{var}}}}}\n"));
                    }
                    None => block.push_str(&format!("{key_str}: {{{{{var}}}}}\n")),
                }
            } else {
                // T1：非敏感 Header 值命中动态形状 → 反向模板化（凭据模板化优先已由分支保证）
                let rendered =
                    super::dynamic::template_value(&val_str).unwrap_or_else(|| val_str.clone());
                block.push_str(&format!("{}: {}\n", key_str, rendered));
            }
        }

        // 4. Body
        if let Some(body) = entry.request.body.as_ref().filter(|b| !b.trim().is_empty()) {
            // Body 暂不做键级模板化（Header/URL query 已模板化）：含敏感键时落盘前明确警告
            let body_sensitive = ["password", "passwd", "secret", "api_key", "apikey", "token"]
                .iter()
                .any(|k| body.to_ascii_lowercase().contains(k));
            if body_sensitive {
                eprintln!(
                    "{} 请求体疑似包含明文凭据（Header/URL query 已模板化，Body 未模板化）——落盘前请手工替换为 {{{{变量}}}}",
                    "⚠".yellow()
                );
            }
            block.push('\n');
            // Try to pretty print JSON
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
                // T1：JSON 字符串叶子命中动态形状 → 反向模板化（敏感键跳过，凭据警告语义不变）；
                // T2 溯源命中优先于形状（PRD 管线序）
                let templatized = Self::templatize_dynamic_leaves(&json, None, traced, policy);
                if let Ok(pretty) = serde_json::to_string_pretty(&templatized) {
                    block.push_str(&pretty);
                } else {
                    block.push_str(body);
                }
            } else {
                block.push_str(body);
            }
            block.push('\n');
        }

        // 5. Assertions (Metadata)
        // Add implicit assertion for status code
        block.push_str(&format!(
            "\n# @assert status == {}\n",
            entry.response.status
        ));

        block
    }

    fn should_skip_header(name: &str) -> bool {
        let name_lower = name.to_lowercase();
        matches!(
            name_lower.as_str(),
            "content-length" | "host" | "connection" | "accept-encoding" | "user-agent"
        )
    }

    /// 语义化命名：`GET /users` + `get_users`；同文件冲突时追加序号（`get_users_2`）。
    /// slug 仅取 URL path（排除 query，避免敏感参数混入名称），非字母数字转下划线。
    fn semantic_title_and_name(
        entry: &HistoryEntry,
        used: &mut HashMap<String, usize>,
    ) -> (String, String) {
        let method = entry.request.method.to_ascii_uppercase();
        let path = url::Url::parse(&entry.request.url)
            .map(|u| u.path().to_string())
            .unwrap_or_default();

        let title = if path.is_empty() || path == "/" {
            method.clone()
        } else {
            format!("{method} {path}")
        };

        let slug: String = path
            .trim_start_matches('/')
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect();
        let mut slug = slug.trim_matches('_').to_string();
        if slug.is_empty() {
            slug = method.to_lowercase();
        } else {
            slug = format!("{}_{}", method.to_lowercase(), slug);
        }
        slug.truncate(48);
        while slug.ends_with('_') {
            slug.pop();
        }

        let count = used.entry(slug.clone()).or_insert(0);
        *count += 1;
        let name = if *count == 1 {
            slug
        } else {
            format!("{slug}_{count}")
        };
        (title, name)
    }

    /// Header/Query 键名转变量名：小写、非字母数字转下划线、压缩连续下划线并去除首尾下划线（若为空则回退至 "secret"）
    fn var_name_from_key(key: &str) -> String {
        let mut s = String::with_capacity(key.len());
        let mut prev_underscore = false;
        for c in key.chars() {
            if c.is_ascii_alphanumeric() {
                s.push(c.to_ascii_lowercase());
                prev_underscore = false;
            } else if !prev_underscore {
                s.push('_');
                prev_underscore = true;
            }
        }
        let trimmed = s.trim_matches('_');
        if trimmed.is_empty() {
            "secret".to_string()
        } else {
            trimmed.to_string()
        }
    }

    /// 已识别认证 scheme（Bearer/Basic/Digest/Token）时返回它，变量仅承载凭证本体
    fn split_auth_scheme(value: &str) -> Option<&str> {
        let (scheme, rest) = value.split_once(' ')?;
        let known = matches!(
            scheme.to_ascii_uppercase().as_str(),
            "BEARER" | "BASIC" | "DIGEST" | "TOKEN"
        );
        (known && !rest.trim().is_empty()).then_some(scheme)
    }

    /// 将 origin（scheme://host[:port]）改写为 `{{base_url}}`；变量名沿用
    /// `rupost.toml` 模板约定。复用全库单一事实来源 `UrlTemplatizer::templatize_origin`。
    fn templatize_host_in_url(raw: &str, variables: &mut BTreeSet<String>) -> String {
        let templatized = crate::http::url::UrlTemplatizer::templatize_origin(raw, "{{base_url}}");
        if templatized != raw && templatized.contains("{{base_url}}") {
            variables.insert("base_url".to_string());
        }
        templatized
    }

    /// T1：递归模板化 JSON 字符串叶子（动态形状 → `{{$...}}`）；
    /// T2 最高优先（评审 F1③）：溯源命中值即使位于敏感键之下也 → `{{capture}}`
    /// （字面量明文被替换，严格更安全）；无命中时敏感键早退维持凭据语义
    fn templatize_dynamic_leaves(
        value: &serde_json::Value,
        key: Option<&str>,
        traced: Option<&super::trace::TracePlan>,
        policy: Option<&crate::security::redact::RedactionPolicy>,
    ) -> serde_json::Value {
        use serde_json::Value;
        if let Value::String(s) = value
            && let Some(plan) = traced
            && let Some(l) = plan.links.iter().find(|l| l.value == *s)
        {
            return Value::String(format!("{{{{{}}}}}", l.capture_name));
        }
        if let Some(k) = key
            && is_key_sensitive(k, policy)
        {
            return value.clone();
        }
        match value {
            Value::String(s) => {
                // T1：非敏感叶子命中动态形状 → 反向模板化
                super::dynamic::template_value(s)
                    .map(Value::String)
                    .unwrap_or_else(|| value.clone())
            }
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            Self::templatize_dynamic_leaves(v, Some(k), traced, policy),
                        )
                    })
                    .collect(),
            ),
            Value::Array(items) => Value::Array(
                items
                    .iter()
                    .map(|v| Self::templatize_dynamic_leaves(v, key, traced, policy))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    /// 结构化切分 URL 并模板化 Query 参数与 SPA Fragment 参数中的敏感键。
    ///
    /// 核心保障：
    /// 1. 标准 URL：`https://api.com/items?page=1&token=sec#my-anchor`
    ///    - `token` 正确模板化为 `{{token}}`；
    ///    - 锚点 `#my-anchor` 100% 原样保留，绝不丢失。
    /// 2. SPA 路由：`https://app.com/#/login?token=sec`
    ///    - 识别 fragment 中的 query 部分并实施模板化。
    /// 3. 复合形态：标准 Query 与 Fragment Query 并存时，两部分同时脱敏，杜绝安全绕过。
    /// 4. 支持前置 `{{base_url}}` 模板变量占位符，不发生解析降级。
    fn variabilize_url(
        raw: &str,
        variables: &mut BTreeSet<String>,
        traced: Option<&super::trace::TracePlan>,
        policy: Option<&crate::security::redact::RedactionPolicy>,
    ) -> String {
        let (before_hash, after_hash) = match raw.split_once('#') {
            Some((bh, ah)) => (bh, Some(ah)),
            None => (raw, None),
        };

        let mut changed = false;

        // 1. 处理主体 URL（# 前部分）的标准 Query 参数
        let rebuilt_before = if let Some((base_path, query_str)) = before_hash.split_once('?') {
            let (rebuilt_query, query_changed) =
                Self::variabilize_query_pairs(query_str, variables, traced, policy);
            if query_changed {
                changed = true;
                format!("{base_path}?{rebuilt_query}")
            } else {
                before_hash.to_string()
            }
        } else {
            before_hash.to_string()
        };

        // 2. 处理 Fragment（# 后部分），若包含问号则作为 SPA Query 进行脱敏
        let rebuilt_after = after_hash.map(|ah| {
            if let Some((spa_path, spa_query)) = ah.split_once('?') {
                let (rebuilt_spa_query, spa_changed) =
                    Self::variabilize_query_pairs(spa_query, variables, traced, policy);
                if spa_changed {
                    changed = true;
                    format!("{spa_path}?{rebuilt_spa_query}")
                } else {
                    ah.to_string()
                }
            } else {
                ah.to_string()
            }
        });

        if !changed {
            return raw.to_string();
        }

        match rebuilt_after {
            Some(ah) => format!("{rebuilt_before}#{ah}"),
            None => rebuilt_before,
        }
    }

    /// 对形如 `k1=v1&k2=v2` 的 query 字符串进行结构化反向模板化
    /// 返回 `(模板化后的 query 串, 是否发生变化)`
    fn variabilize_query_pairs(
        query: &str,
        variables: &mut BTreeSet<String>,
        traced: Option<&super::trace::TracePlan>,
        policy: Option<&crate::security::redact::RedactionPolicy>,
    ) -> (String, bool) {
        let mut templated = false;
        let rebuilt: Vec<String> = query
            .split('&')
            .map(|pair| match pair.split_once('=') {
                Some((k, _v)) if is_key_sensitive(k, policy) => {
                    templated = true;
                    let var = Self::var_name_from_key(k);
                    variables.insert(var.clone());
                    format!("{k}={{{{{var}}}}}")
                }
                Some((k, v)) => {
                    // T2 最高优先：溯源命中 query 值 → {{capture}}
                    if let Some(plan) = traced
                        && let Some(l) = plan.links.iter().find(|l| l.value == *v)
                    {
                        templated = true;
                        return format!("{k}={{{{{}}}}}", l.capture_name);
                    }
                    // T1：非敏感 query 值命中动态形状 → 反向模板化
                    match super::dynamic::template_value(v) {
                        Some(t) => {
                            templated = true;
                            format!("{k}={t}")
                        }
                        None => pair.to_string(),
                    }
                }
                None => pair.to_string(),
            })
            .collect();

        (rebuilt.join("&"), templated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_var_name_from_key_sanitization() {
        assert_eq!(HttpGenerator::var_name_from_key("X-Api-Key"), "x_api_key");
        assert_eq!(
            HttpGenerator::var_name_from_key("Authorization"),
            "authorization"
        );
        assert_eq!(
            HttpGenerator::var_name_from_key("---X--Api--Key---"),
            "x_api_key"
        );
        assert_eq!(
            HttpGenerator::var_name_from_key("api.token.secret"),
            "api_token_secret"
        );
        assert_eq!(HttpGenerator::var_name_from_key("---$$$---"), "secret");
        assert_eq!(HttpGenerator::var_name_from_key(""), "secret");
    }

    #[test]
    fn test_generator_with_custom_redaction_policy() {
        use crate::history::model::{HistoryEntry, RequestSnapshot, ResponseMeta};
        use crate::security::redact::RedactionPolicy;
        use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("x-custom-signature"),
            HeaderValue::from_static("sig-123456"),
        );
        headers.insert(
            HeaderName::from_static("x-normal-header"),
            HeaderValue::from_static("normal-val"),
        );

        let entry = HistoryEntry {
            id: "entry-1".to_string(),
            timestamp: chrono::Utc::now(),
            duration_ms: 10,
            source: None,
            request: RequestSnapshot {
                timeout: None,
                method: "GET".to_string(),
                url: "https://api.com/users?x-tenant-token=tok999&limit=10".to_string(),
                headers,
                body: None,
            },
            response: ResponseMeta {
                status: 200,
                headers: HeaderMap::new(),
                body: None,
            },
        };

        // 自定义策略：把 x-custom-signature 和 x-tenant-token 加入 extra_keys
        let policy =
            RedactionPolicy::new(["x-custom-signature", "x-tenant-token"], Vec::<&str>::new());
        let (output, vars) =
            HttpGenerator::generate_with_policy(&[entry], false, Some(&policy), false).unwrap();

        // 验证 Header 与 Query 中的自定义凭据均被模板化
        assert!(output.contains("x-custom-signature: {{x_custom_signature}}"));
        assert!(output.contains("x-normal-header: normal-val"));
        assert!(output.contains("x-tenant-token={{x_tenant_token}}"));
        assert!(output.contains("limit=10"));
        assert!(vars.contains(&"x_custom_signature".to_string()));
        assert!(vars.contains(&"x_tenant_token".to_string()));
    }

    #[test]
    fn test_variabilize_url_with_fragment_preservation() {
        let mut vars = BTreeSet::new();

        // 场景 A：敏感键位于末尾，后接普通锚点 —— 锚点必须 100% 完整保留
        let url_a = "https://api.com/items?limit=10&access_token=secret999#section-details";
        let out_a = HttpGenerator::variabilize_url(url_a, &mut vars, None, None);
        assert_eq!(
            out_a,
            "https://api.com/items?limit=10&access_token={{access_token}}#section-details"
        );
        assert!(vars.contains("access_token"));

        // 场景 B：前置已被 templatize_host 改写为 {{base_url}}，同样无缝保留锚点
        let url_b = "{{base_url}}/items?token=secret888#quick-start";
        let out_b = HttpGenerator::variabilize_url(url_b, &mut vars, None, None);
        assert_eq!(out_b, "{{base_url}}/items?token={{token}}#quick-start");

        // 场景 C：SPA 路由形态（# 在 ? 之前）—— 参数正常模板化，SPA 路由路径原样保留
        let url_c = "https://app.com/#/dashboard?auth_token=secret777&mode=dark";
        let out_c = HttpGenerator::variabilize_url(url_c, &mut vars, None, None);
        assert_eq!(
            out_c,
            "https://app.com/#/dashboard?auth_token={{auth_token}}&mode=dark"
        );

        // 场景 D：纯锚点无敏感键，保持零修改原样返回
        let url_d = "https://docs.com/guide#keyboard-shortcuts";
        let out_d = HttpGenerator::variabilize_url(url_d, &mut vars, None, None);
        assert_eq!(out_d, url_d);

        // 场景 E（安全漏洞防护）：主体 Query 含敏感凭据，Fragment 亦包含带参锚点 —— 绝不发生凭据泄漏
        let url_e = "https://api.com/v1/auth?secret_key=SUPER_SECRET_TOKEN#heading?section=2";
        let out_e = HttpGenerator::variabilize_url(url_e, &mut vars, None, None);
        assert_eq!(
            out_e,
            "https://api.com/v1/auth?secret_key={{secret_key}}#heading?section=2"
        );
        assert!(vars.contains("secret_key"));
        assert!(!out_e.contains("SUPER_SECRET_TOKEN"));

        // 场景 F：主体与 Fragment 内部同时含有敏感凭据 —— 双侧同时脱敏
        let url_f = "https://api.com/login?token=sec1#spa/path?access_token=sec2";
        let out_f = HttpGenerator::variabilize_url(url_f, &mut vars, None, None);
        assert_eq!(
            out_f,
            "https://api.com/login?token={{token}}#spa/path?access_token={{access_token}}"
        );
        assert!(vars.contains("token"));
        assert!(vars.contains("access_token"));
        assert!(!out_f.contains("sec1"));
        assert!(!out_f.contains("sec2"));
    }
}
