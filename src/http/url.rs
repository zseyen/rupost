#![allow(clippy::manual_strip)]
use crate::variable::VariableContext;
use crate::{Result, RupostError};

/// 判断是否以合法 scheme 开头（RFC 3986: ALPHA *( ALPHA / DIGIT / "+" / "-" / "." ) 后紧跟 "://"）。
/// 不能用 `contains("://")`：`/oauth/cb?redirect=https://x` 这类 query 内含 URL 的相对路径会被误判为绝对地址。
fn has_scheme_prefix(url: &str) -> bool {
    let Some(pos) = url.find("://") else {
        return false;
    };
    let scheme = &url[..pos];
    let mut chars = scheme.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

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
    if has_scheme_prefix(raw_url) {
        return Ok(raw_url.to_string());
    }

    // 2. 处理冒号 localhost 缩写
    if let Some(suffix) = raw_url.strip_prefix(':') {
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
            if has_scheme_prefix(f) {
                Ok(join_paths(&[f, relative_url]))
            } else {
                Ok(join_paths(&[g.as_str(), f, relative_url]))
            }
        }
        (Some(g), None) => Ok(join_paths(&[g.as_str(), relative_url])),
        (None, Some(f)) => {
            if has_scheme_prefix(f) {
                Ok(join_paths(&[f, relative_url]))
            } else {
                Err(RupostError::BaseUrlNotConfigured)
            }
        }
        (None, None) => Err(RupostError::BaseUrlNotConfigured),
    }
}

/// 自由输入（CLI 参数 / TUI 快速输入）的归一化结果。
/// 同一谓词服务 CLI 与 TUI，保证两处语义一致（US-T3）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormalizedInput {
    /// 携带合法 scheme（RFC 3986）的绝对地址，原样返回
    Absolute(String),
    /// `:3000/x` 冒号 localhost 缩写 → `http://localhost:3000/x`
    LocalhostShorthand(String),
    /// 独立主机（含端口或域名，如 `api.github.com/users`）→ 补全 scheme
    HostName(String),
    /// 非 URL：按文件名处理
    NotUrl(String),
}

/// 把自由文本输入归一化为绝对 URL 或文件名判定。
/// 冒号缩写复用 resolve_final_url 的 CLI 语义；独立主机判定与 CLI is_independent 同一规则
/// （含空格、`.http`/`.md` 后缀的输入按文件名处理——这是 TUI 快速新建的文件名通道）。
pub fn normalize_input_url(input: &str, default_scheme: &str) -> NormalizedInput {
    if has_scheme_prefix(input) {
        return NormalizedInput::Absolute(input.to_string());
    }
    if input.starts_with(':') {
        let resolved = resolve_final_url(input, &VariableContext::new(), "http", Some("cli"), None)
            .unwrap_or_else(|_| format!("http://localhost{input}"));
        return NormalizedInput::LocalhostShorthand(resolved);
    }
    let first = input.split('/').next().unwrap_or("");
    let is_independent = first.contains('.')
        || first
            .find(':')
            .map(|p| {
                let after = &first[p + 1..];
                !after.is_empty() && after.chars().all(|c| c.is_ascii_digit())
            })
            .unwrap_or(false);
    let file_like = input.contains(' ') || input.ends_with(".http") || input.ends_with(".md");
    if is_independent && !file_like {
        // 本地主机没有 TLS，强行补默认 scheme 会制造必败请求
        let scheme = if first.starts_with("localhost") || first.starts_with("127.") {
            "http"
        } else {
            default_scheme
        };
        return NormalizedInput::HostName(format!("{scheme}://{input}"));
    }
    NormalizedInput::NotUrl(input.to_string())
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
    fn test_normalize_input_url_decision_table() {
        use NormalizedInput::*;
        // (输入, 期望)——判定表与设计文档 v3.3 决策表一致
        let cases: Vec<(&str, NormalizedInput)> = vec![
            ("https://a.com/x", Absolute("https://a.com/x".into())),
            ("ws://a.com", Absolute("ws://a.com".into())),
            ("wss://a.com/ws?t=1", Absolute("wss://a.com/ws?t=1".into())),
            (
                ":3000/x",
                LocalhostShorthand("http://localhost:3000/x".into()),
            ),
            (
                ":/health",
                LocalhostShorthand("http://localhost/health".into()),
            ),
            (
                "localhost:3000/x",
                HostName("http://localhost:3000/x".into()),
            ),
            (
                "api.github.com/users",
                HostName("https://api.github.com/users".into()),
            ),
            // 与 CLI is_independent 同规则：首段含点即主机（v1.2/users 同 CLI 判为主机）
            ("v1.2/users", HostName("https://v1.2/users".into())),
            ("my.file", HostName("https://my.file".into())),
            // 文件名通道
            ("tests/auth.http", NotUrl("tests/auth.http".into())),
            ("auth", NotUrl("auth".into())),
            ("my notes", NotUrl("my notes".into())),
            ("/users", NotUrl("/users".into())),
        ];
        for (input, expected) in cases {
            assert_eq!(
                normalize_input_url(input, "https"),
                expected,
                "input={input}"
            );
        }
    }

    #[test]
    fn test_relative_path_with_url_in_query_is_not_absolute() {
        // 回归：此前 contains("://") 把 query 里含 URL 的相对路径当成绝对地址原样返回，
        // reqwest 随后报 "relative URL without a base"，OAuth 回调类参数极常见。
        let mut ctx = VariableContext::new();
        ctx.insert("base_url", "https://api.example.com");

        let resolved = resolve_final_url(
            "/oauth/callback?redirect=https://app.example.com/done",
            &ctx,
            "http",
            Some("file"),
            None,
        )
        .unwrap();
        assert_eq!(
            resolved,
            "https://api.example.com/oauth/callback?redirect=https://app.example.com/done"
        );

        // 真正的绝对地址（含 query 里再带 URL）仍被识别为绝对
        assert!(has_scheme_prefix("https://a.com/x?u=http://b.com"));
        assert!(has_scheme_prefix("wss://a.com/ws"));
        // 非 scheme 形态一律不算绝对
        assert!(!has_scheme_prefix("/path?u=http://b.com"));
        assert!(!has_scheme_prefix("example.com/?x=http://a"));
        assert!(!has_scheme_prefix("localhost:8080/x"));
        assert!(!has_scheme_prefix("no-scheme-here"));
    }

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

    #[test]
    fn test_resolve_final_url_edge_cases() {
        let mut ctx = VariableContext::new();
        ctx.insert("baseUrl", "http://by-camelcase.com");

        // 1. 验证 camelCase 形式的 baseUrl 获取优先级
        assert_eq!(
            resolve_final_url("/users", &ctx, "http", Some("file.http"), None).unwrap(),
            "http://by-camelcase.com/users"
        );

        // 2. 验证 base_url 优于 baseUrl
        ctx.insert("base_url", "http://by-snakecase.com");
        assert_eq!(
            resolve_final_url("/users", &ctx, "http", Some("file.http"), None).unwrap(),
            "http://by-snakecase.com/users"
        );

        // 3. 验证 BASE_URL 的优先级
        let mut ctx2 = VariableContext::new();
        ctx2.insert("BASE_URL", "http://by-uppercase.com");
        assert_eq!(
            resolve_final_url("/users", &ctx2, "http", Some("file.http"), None).unwrap(),
            "http://by-uppercase.com/users"
        );

        // 4. raw_url 恰好为 "/"
        assert_eq!(
            resolve_final_url("/", &ctx, "http", Some("file.http"), None).unwrap(),
            "http://by-snakecase.com"
        );

        // 5. WebSocket 协议原样返回
        assert_eq!(
            resolve_final_url("ws://example.com/ws", &ctx, "http", Some("file.http"), None)
                .unwrap(),
            "ws://example.com/ws"
        );
        assert_eq!(
            resolve_final_url(
                "wss://example.com/ws",
                &ctx,
                "http",
                Some("file.http"),
                None
            )
            .unwrap(),
            "wss://example.com/ws"
        );
    }
}
