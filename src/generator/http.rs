use crate::Result;
use crate::exporter::redact::is_sensitive_key;
use crate::history::model::HistoryEntry;
use std::collections::BTreeSet;

pub struct HttpGenerator;

impl HttpGenerator {
    /// Convert a list of history entries to .http file content
    pub fn generate(entries: &[HistoryEntry]) -> Result<String> {
        Ok(Self::generate_with_variables(entries)?.0)
    }

    /// Convert history entries to .http content, plus the sorted names of all
    /// variables referenced by templated credentials.
    ///
    /// 敏感凭据一律模板化为 `{{variable}}` 引用（绝不内联原值），用户经四层级联
    /// （`--var` > 系统环境变量 > `.env` > `rupost.toml`）任一渠道提供真值即可运行；
    /// 与 `export` 的 `[REDACTED]` 脱敏分工：export 给人看，generate 给机器跑。
    pub fn generate_with_variables(entries: &[HistoryEntry]) -> Result<(String, Vec<String>)> {
        let mut output = String::new();
        let mut variables = BTreeSet::new();

        for (i, entry) in entries.iter().enumerate() {
            if i > 0 {
                output.push_str("\n\n");
            }
            output.push_str(&Self::format_entry(entry, &mut variables));
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

    fn format_entry(entry: &HistoryEntry, variables: &mut BTreeSet<String>) -> String {
        let mut block = String::new();

        // 1. Comment / Name
        // Generate a name like: "GET /api/users - 10:23:45"

        block.push_str(&format!(
            "### Request {}\n",
            entry.id.chars().take(8).collect::<String>()
        ));
        block.push_str(&format!(
            "# @name req_{}_{}\n",
            entry.timestamp.timestamp(),
            i8::abs(entry.id.as_bytes()[0] as i8)
        ));

        // 1.5 Timeout（快照记录了 @timeout 时还原该指令）
        if let Some(t) = entry.request.timeout {
            block.push_str(&format!("# @timeout {}ms\n", t.as_millis()));
        }

        // 2. Request Line（URL query 中的敏感键值模板化为变量）
        block.push_str(&format!(
            "{} {}\n",
            entry.request.method,
            Self::variablize_url(&entry.request.url, variables)
        ));

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
