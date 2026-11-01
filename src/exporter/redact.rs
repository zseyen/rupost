//! 敏感凭据脱敏工具

/// 判断 Header 键名是否属于敏感信息
pub fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "authorization"
            | "proxy-authorization"
            | "x-api-key"
            | "api-key"
            | "x-auth-token"
            | "cookie"
            | "set-cookie"
    ) || lower.contains("secret")
        || lower.contains("token")
        || lower.contains("password")
        || lower.contains("apikey")
        || lower.contains("api_key")
}

/// 对 Header 凭据值进行掩码脱敏
///
/// 若包含常见的认证 scheme 前缀（如 Bearer / Basic / Digest / Token），保留前缀便于识别认证模式
pub fn redact_header_value(key: &str, value: &str) -> String {
    if !is_sensitive_key(key) {
        return value.to_string();
    }
    if let Some((scheme, rest)) = value.split_once(' ')
        && !rest.trim().is_empty()
        && matches!(
            scheme.to_ascii_uppercase().as_str(),
            "BEARER" | "BASIC" | "DIGEST" | "TOKEN"
        )
    {
        return format!("{scheme} [REDACTED]");
    }
    "[REDACTED]".to_string()
}

/// 脱敏 URL Query 参数中的敏感键值
///
/// 示例：`https://api.com?api_key=secret123&page=1` -> `https://api.com/?api_key=%5BREDACTED%5D&page=1`
pub fn redact_url(raw_url: &str) -> String {
    let Ok(mut parsed) = url::Url::parse(raw_url) else {
        return raw_url.to_string();
    };
    let pairs: Vec<(String, String)> = parsed
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    if pairs.is_empty() || !pairs.iter().any(|(k, _)| is_sensitive_key(k)) {
        return raw_url.to_string();
    }

    {
        let mut qp = parsed.query_pairs_mut();
        qp.clear();
        for (k, v) in pairs {
            if is_sensitive_key(&k) {
                qp.append_pair(&k, "[REDACTED]");
            } else {
                qp.append_pair(&k, &v);
            }
        }
    }
    parsed.into()
}

#[cfg(test)]
mod tests {
    use super::*;

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
