use crate::Result;
use crate::exporter::redact::is_sensitive_key;
use crate::history::model::HistoryEntry;
use colored::Colorize;
use std::collections::{BTreeSet, HashMap};

pub struct HttpGenerator;

impl HttpGenerator {
    /// Convert a list of history entries to .http file content
    pub fn generate(entries: &[HistoryEntry]) -> Result<String> {
        Ok(Self::generate_with_variables(entries, false)?.0)
    }

    /// Convert history entries to .http content, plus the sorted names of all
    /// variables referenced by templated credentials.
    ///
    /// 敏感凭据一律模板化为 `{{variable}}` 引用（绝不内联原值），用户经四层级联
    /// （`--var` > 系统环境变量 > `.env` > `rupost.toml`）任一渠道提供真值即可运行；
    /// 与 `export` 的 `[REDACTED]` 脱敏分工：export 给人看，generate 给机器跑。
    ///
    /// `templatize_host` 为 true 时，将请求 origin（scheme://host[:port]）改写为
    /// `{{base_url}}`，使同一文件经变量级联跨环境运行。
    pub fn generate_with_variables(
        entries: &[HistoryEntry],
        templatize_host: bool,
    ) -> Result<(String, Vec<String>)> {
        let mut output = String::new();
        let mut variables = BTreeSet::new();
        let mut used_names: HashMap<String, usize> = HashMap::new();

        // T2：链式值溯源（时间序即拓扑序；上游产出 → 下游 {{capture}} 重建数据流）
        let trace_plan = super::trace::build_trace_plan(entries);
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

    fn format_entry(
        entry: &HistoryEntry,
        templatize_host: bool,
        variables: &mut BTreeSet<String>,
        used_names: &mut HashMap<String, usize>,
        captures: Option<&[&super::trace::TracedLink]>,
        traced: Option<&super::trace::TracePlan>,
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
            )
        } else {
            Self::variabilize_url(&entry.request.url, variables, traced)
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
            if is_sensitive_key(key_str) {
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
                let rendered = super::dynamic::template_value(&val_str)
                    .unwrap_or_else(|| val_str.clone());
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
                let templatized = Self::templatize_dynamic_leaves(&json, None, traced);
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

    /// Header/Query 键名转变量名：小写、非字母数字转下划线（`X-Api-Key` -> `x_api_key`）
    fn var_name_from_key(key: &str) -> String {
        key.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect()
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
    /// `rupost.toml` 模板约定。其余部分逐字保留。
    fn templatize_host_in_url(raw: &str, variables: &mut BTreeSet<String>) -> String {
        let Ok(parsed) = url::Url::parse(raw) else {
            return raw.to_string();
        };
        let Some(host) = parsed.host_str() else {
            return raw.to_string();
        };
        if host.is_empty() {
            return raw.to_string();
        }
        let mut origin = format!("{}://{}", parsed.scheme(), host);
        if let Some(port) = parsed.port() {
            origin.push_str(&format!(":{port}"));
        }
        match raw.find(&origin) {
            Some(pos) => {
                variables.insert("base_url".to_string());
                let mut out = String::with_capacity(raw.len());
                out.push_str(&raw[..pos]);
                out.push_str("{{base_url}}");
                out.push_str(&raw[pos + origin.len()..]);
                out
            }
            None => raw.to_string(),
        }
    }

    /// T1：递归模板化 JSON 字符串叶子（动态形状 → `{{$...}}`）；
    /// T2 最高优先（评审 F1③）：溯源命中值即使位于敏感键之下也 → `{{capture}}`
    /// （字面量明文被替换，严格更安全）；无命中时敏感键早退维持凭据语义
    fn templatize_dynamic_leaves(
        value: &serde_json::Value,
        key: Option<&str>,
        traced: Option<&super::trace::TracePlan>,
    ) -> serde_json::Value {
        use serde_json::Value;
        if let Value::String(s) = value
            && let Some(plan) = traced
            && let Some(l) = plan.links.iter().find(|l| l.value == *s)
        {
            return Value::String(format!("{{{{{}}}}}", l.capture_name));
        }
        if let Some(k) = key
            && crate::exporter::redact::is_sensitive_key(k)
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
                            Self::templatize_dynamic_leaves(v, Some(k), traced),
                        )
                    })
                    .collect(),
            ),
            Value::Array(items) => Value::Array(
                items
                    .iter()
                    .map(|v| Self::templatize_dynamic_leaves(v, key, traced))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    /// URL query 中的敏感键值模板化为 `{{变量}}`；溯源命中值（T2，最高优先）→ `{{capture}}`；
    /// 其余 T1 动态形状反向模板化；其余部分逐字保留（不做重编码）。
    /// 已知限制：URL 编码形态的值（%3A 等）不参与溯源/形状识别。
    fn variabilize_url(
        raw: &str,
        variables: &mut BTreeSet<String>,
        traced: Option<&super::trace::TracePlan>,
    ) -> String {
        let Some((base, query)) = raw.split_once('?') else {
            return raw.to_string();
        };
        let mut templated = false;
        let rebuilt: Vec<String> = query
            .split('&')
            .map(|pair| match pair.split_once('=') {
                Some((k, _v)) if is_sensitive_key(k) => {
                    templated = true;
                    let var = Self::var_name_from_key(k);
                    variables.insert(var.clone());
                    format!("{k}={{{{{var}}}}}")
                }
                Some((k, v)) => {
                    // T2 最高优先：溯源命中 query 值 → {{capture}}（管线序 T2>T1>凭据豁免）
                    if let Some(plan) = traced
                        && let Some(l) = plan.links.iter().find(|l| l.value == *v)
                    {
                        templated = true;
                        return format!("{k}={{{{{}}}}}", l.capture_name);
                    }
                    // T1：非敏感 query 值命中动态形状 → 反向模板化（不进变量符号表）
                    match super::dynamic::template_value(v) {
                        Some(t) => {
                            templated = true;
                            format!("{k}={t}")
                        }
                        None => pair.to_string(),
                    }
                }
                _ => pair.to_string(),
            })
            .collect();
        if templated {
            format!("{base}?{}", rebuilt.join("&"))
        } else {
            raw.to_string()
        }
    }
}
