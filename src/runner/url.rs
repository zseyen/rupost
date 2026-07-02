use crate::variable::VariableContext;
use crate::{Result, RupostError};

/// 智能解析最终请求 URL，处理绝对/相对路径和本地/CLI 协议快捷简写
pub fn resolve_final_url(
    raw_url: &str,
    context: &VariableContext,
    default_scheme: &str,
    source: Option<&str>,
    file_base_path: Option<&str>,
) -> Result<String> {
    let is_cli = source == Some("cli");

    // 1. 处理绝对 URL
    if raw_url.contains("://") || raw_url.starts_with("http://") || raw_url.starts_with("https://")
    {
        return Ok(raw_url.to_string());
    }

    // 2. 处理冒号 localhost 缩写
    if raw_url.starts_with(':') {
        let suffix = &raw_url[1..];
        let base = if is_cli {
            format!("{}://localhost", default_scheme)
        } else {
            "http://localhost".to_string()
        };

        let final_url = if suffix
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
        {
            format!("{}:{}", base, suffix)
        } else {
            format!("{}{}", base, suffix)
        };
        return Ok(final_url);
    }

    // 3. 处理独立主机判断（智能协议补全）
    if is_cli {
        let first_segment = raw_url.split('/').next().unwrap_or("");
        let is_independent = if first_segment.contains('.') {
            true
        } else if let Some(colon_pos) = first_segment.find(':') {
            let after_colon = &first_segment[colon_pos + 1..];
            !after_colon.is_empty() && after_colon.chars().all(|c| c.is_ascii_digit())
        } else {
            false
        };

        if is_independent {
            return Ok(format!("{}://{}", default_scheme, raw_url));
        }
    }

    // 4. 作为相对路径，进行分层拼接
    let relative_url = if raw_url == "/" { "" } else { raw_url };

    let global_base = context
        .get("base_url")
        .or_else(|| context.get("baseUrl"))
        .or_else(|| context.get("BASE_URL"));

    match (global_base, file_base_path) {
        (Some(g), Some(f)) => {
            if f.contains("://") {
                Ok(join_paths(&[f, relative_url]))
            } else {
                Ok(join_paths(&[g.as_str(), f, relative_url]))
            }
        }
        (Some(g), None) => Ok(join_paths(&[g.as_str(), relative_url])),
        (None, Some(f)) => {
            if f.contains("://") {
                Ok(join_paths(&[f, relative_url]))
            } else {
                Err(RupostError::BaseUrlNotConfigured)
            }
        }
        (None, None) => Err(RupostError::BaseUrlNotConfigured),
    }
}

pub fn join_paths(parts: &[&str]) -> String {
    let mut result = String::new();
    for part in parts {
        let part_trimmed = part.trim();
        if part_trimmed.is_empty() {
            continue;
        }
        if result.is_empty() {
            result.push_str(part_trimmed);
        } else {
            let has_slash_end = result.ends_with('/');
            let has_slash_start = part_trimmed.starts_with('/');
            match (has_slash_end, has_slash_start) {
                (true, true) => {
                    result.push_str(&part_trimmed[1..]);
                }
                (false, false) => {
                    result.push('/');
                    result.push_str(part_trimmed);
                }
                _ => {
                    result.push_str(part_trimmed);
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::variable::VariableContext;

    #[test]
    fn test_join_paths() {
        assert_eq!(
            join_paths(&["http://example.com", "v1", "users"]),
            "http://example.com/v1/users"
        );
        assert_eq!(
            join_paths(&["http://example.com/", "/v1/", "/users"]),
            "http://example.com/v1/users"
        );
        assert_eq!(
            join_paths(&["http://example.com", "/users"]),
            "http://example.com/users"
        );
        assert_eq!(
            join_paths(&["http://example.com/", "users"]),
            "http://example.com/users"
        );
        assert_eq!(join_paths(&["", "users"]), "users");
    }

    #[test]
    fn test_resolve_final_url_absolute() {
        let ctx = VariableContext::new();
        assert_eq!(
            resolve_final_url("https://example.com/users", &ctx, "http", None, None).unwrap(),
            "https://example.com/users"
        );
        assert_eq!(
            resolve_final_url("http://example.com/users", &ctx, "http", Some("cli"), None).unwrap(),
            "http://example.com/users"
        );
    }

    #[test]
    fn test_resolve_final_url_cli_localhost_shorthand() {
        let ctx = VariableContext::new();
        assert_eq!(
            resolve_final_url(":3000/users", &ctx, "http", Some("cli"), None).unwrap(),
            "http://localhost:3000/users"
        );
        assert_eq!(
            resolve_final_url(":/health", &ctx, "https", Some("cli"), None).unwrap(),
            "https://localhost/health"
        );
        assert_eq!(
            resolve_final_url(":", &ctx, "http", Some("cli"), None).unwrap(),
            "http://localhost"
        );
    }

    #[test]
    fn test_resolve_final_url_doc_localhost_shorthand() {
        let ctx = VariableContext::new();
        // 场景 B (Document)：首部冒号始终替换为 http://localhost
        assert_eq!(
            resolve_final_url(":3000/users", &ctx, "https", Some("file.http"), None).unwrap(),
            "http://localhost:3000/users"
        );
        assert_eq!(
            resolve_final_url(":/health", &ctx, "https", None, None).unwrap(),
            "http://localhost/health"
        );
    }

    #[test]
    fn test_resolve_final_url_cli_independent_host() {
        let ctx = VariableContext::new();
        // 域名场景
        assert_eq!(
            resolve_final_url("api.github.com/users", &ctx, "http", Some("cli"), None).unwrap(),
            "http://api.github.com/users"
        );
        assert_eq!(
            resolve_final_url("api.github.com/users", &ctx, "https", Some("cli"), None).unwrap(),
            "https://api.github.com/users"
        );
        // localhost:port 场景
        assert_eq!(
            resolve_final_url("localhost:3000/users", &ctx, "http", Some("cli"), None).unwrap(),
            "http://localhost:3000/users"
        );
    }

    #[test]
    fn test_resolve_final_url_doc_independent_host_falls_back_to_relative() {
        let mut ctx = VariableContext::new();
        ctx.insert("base_url", "http://my-proxy.com");
        // 场景 B（文档）下，像 api.github.com/users 这种不带协议的应视作相对路径
        assert_eq!(
            resolve_final_url(
                "api.github.com/users",
                &ctx,
                "http",
                Some("file.http"),
                None
            )
            .unwrap(),
            "http://my-proxy.com/api.github.com/users"
        );
    }

    #[test]
    fn test_resolve_final_url_relative_paths() {
        let mut ctx = VariableContext::new();
        ctx.insert("base_url", "http://api.example.com");

        // 仅全局 base_url
        assert_eq!(
            resolve_final_url("/users", &ctx, "http", Some("file.http"), None).unwrap(),
            "http://api.example.com/users"
        );

        // 全局 base_url + 文件级 base_path
        assert_eq!(
            resolve_final_url("/users", &ctx, "http", Some("file.http"), Some("/v1")).unwrap(),
            "http://api.example.com/v1/users"
        );

        // 无全局 base_url 但文件级 base_path 是绝对路径
        let empty_ctx = VariableContext::new();
        assert_eq!(
            resolve_final_url(
                "/users",
                &empty_ctx,
                "http",
                Some("file.http"),
                Some("https://api.absolute.com/v2")
            )
            .unwrap(),
            "https://api.absolute.com/v2/users"
        );

        // 报错情况：无 base_url 且非绝对
        assert!(resolve_final_url("/users", &empty_ctx, "http", Some("file.http"), None).is_err());
    }
}
