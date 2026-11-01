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

        for (i, entry) in entries.iter().enumerate() {
            if i > 0 {
                output.push_str("\n\n");
            }
            output.push_str(&Self::format_entry(
                entry,
                templatize_host,
                &mut variables,
                &mut used_names,
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
    ) -> String {
        let mut block = String::new();

        // 1. Semantic title / name（method + path，同文件冲突时追加序号）
        let (title, name) = Self::semantic_title_and_name(entry, used_names);
        block.push_str(&format!("### {title}\n"));
        block.push_str(&format!("# @name {name}\n"));

        // 1.5 Timeout（快照记录了 @timeout 时还原该指令）
        if let Some(t) = entry.request.timeout {
            block.push_str(&format!("# @timeout {}ms\n", t.as_millis()));
        }

        // 2. Request Line（可选 origin → {{base_url}}；URL query 敏感键值模板化为变量）
        let rendered_url = if templatize_host {
            Self::variablize_url(
                &Self::templatize_host_in_url(&entry.request.url, variables),
                variables,
            )
        } else {
            Self::variablize_url(&entry.request.url, variables)
        };
        block.push_str(&format!("{} {}\n", entry.request.method, rendered_url));

        // 3. Headers
        for (key, value) in &entry.request.headers {
            let key_str = key.as_str();
            // Skip common auto-headers that shouldn't be hardcoded in tests
            if Self::should_skip_header(key_str) {
                continue;
            }
            if let Ok(val_str) = value.to_str() {
                // 凭据模板化：生成的测试文件严禁落盘明文凭证
                if is_sensitive_key(key_str) {
                    let var = Self::var_name_from_key(key_str);
                    variables.insert(var.clone());
                    match Self::split_auth_scheme(val_str) {
                        Some(scheme) => {
                            block.push_str(&format!("{key_str}: {scheme} {{{{{var}}}}}\n"));
                        }
                        None => block.push_str(&format!("{key_str}: {{{{{var}}}}}\n")),
                    }
                } else {
                    block.push_str(&format!("{}: {}\n", key_str, val_str));
                }
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
                if let Ok(pretty) = serde_json::to_string_pretty(&json) {
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

    /// URL query 中的敏感键值模板化为 `{{变量}}`；其余部分逐字保留（不做重编码）
    fn variablize_url(raw: &str, variables: &mut BTreeSet<String>) -> String {
        let Some((base, query)) = raw.split_once('?') else {
            return raw.to_string();
        };
        let mut templated = false;
        let rebuilt: Vec<String> = query
            .split('&')
            .map(|pair| match pair.split_once('=') {
                Some((k, _)) if is_sensitive_key(k) => {
                    templated = true;
                    let var = Self::var_name_from_key(k);
                    variables.insert(var.clone());
                    format!("{k}={{{{{var}}}}}")
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
