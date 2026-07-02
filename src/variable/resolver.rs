use crate::variable::types::VariableContext;
use regex::{Captures, Regex};
use std::sync::OnceLock;

/// 变量替换器
pub struct VariableResolver;

impl VariableResolver {
    /// 替换文本中的所有 {{variable}} 占位符，支持动态内置变量
    pub fn substitute(text: &str, context: &VariableContext) -> String {
        static VAR_REGEX: OnceLock<Regex> = OnceLock::new();
        let re =
            VAR_REGEX.get_or_init(|| Regex::new(r"\{\{([$a-zA-Z_][a-zA-Z0-9_.]*)\}\}").unwrap());

        re.replace_all(text, |caps: &Captures| {
            let var_name = &caps[1];

            // 处理动态内置变量
            match var_name {
                "$uuid" => uuid::Uuid::new_v4().to_string(),
                "$timestamp" => chrono::Utc::now().timestamp().to_string(),
                "$random_int" => {
                    use rand::Rng;
                    let mut rng = rand::rng();
                    rng.random_range(1..=10000).to_string()
                }
                _ => context.get(var_name).unwrap_or_else(|| caps[0].to_string()),
            }
        })
        .to_string()
    }

    /// 解析并替换系统环境变量 ${VAR}
    pub fn resolve_env_vars(text: &str) -> String {
        static ENV_REGEX: OnceLock<Regex> = OnceLock::new();
        let re = ENV_REGEX.get_or_init(|| Regex::new(r"\$\{([A-Z_][A-Z0-9_]*)\}").unwrap());

        re.replace_all(text, |caps: &Captures| {
            let env_name = &caps[1];
            std::env::var(env_name).unwrap_or_else(|_| caps[0].to_string())
        })
        .to_string()
    }

    /// 完整的变量解析流程：先解析环境变量，再替换自定义变量
    pub fn resolve(text: &str, context: &VariableContext) -> String {
        let with_env = Self::resolve_env_vars(text);
        Self::substitute(&with_env, context)
    }

    /// 渲染并实例化一个 `ParsedRequest`：解析其 URL, Headers, Body 中的所有占位符和环境变量；
    /// 并处理 Base URL 的自动拼接以及全局 User-Agent 的注入。
    pub fn resolve_parsed_request(
        parsed: &mut crate::parser::ParsedRequest,
        context: &mut VariableContext,
    ) {
        // 1. 替换 URL
        parsed.url = Self::resolve(&parsed.url, context);

        // 2. 替换 Headers
        for (_key, value) in &mut parsed.headers {
            *value = Self::resolve(value, context);
        }

        // 3. 检查全局变量 context 中是否提供了 user_agent，如果是且请求中没有显式设置，则追加
        if let Some(ua) = context.get("user_agent") {
            let has_ua = parsed
                .headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("user-agent"));
            if !has_ua {
                parsed.headers.push(("User-Agent".to_string(), ua));
            }
        }

        // 4. 替换 Body
        if let Some(body) = &mut parsed.body {
            *body = Self::resolve(body, context);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_substitute_simple() {
        let mut ctx = VariableContext::new();
        ctx.insert("base_url", "http://localhost:8080");
        ctx.insert("token", "secret-token");

        let input = "{{base_url}}/api/users";
        let output = VariableResolver::substitute(input, &ctx);
        assert_eq!(output, "http://localhost:8080/api/users");
    }

    #[test]
    fn test_substitute_multiple() {
        let mut ctx = VariableContext::new();
        ctx.insert("host", "example.com");
        ctx.insert("port", "8080");
        ctx.insert("path", "api");

        let input = "https://{{host}}:{{port}}/{{path}}/users";
        let output = VariableResolver::substitute(input, &ctx);
        assert_eq!(output, "https://example.com:8080/api/users");
    }

    #[test]
    fn test_substitute_missing_variable() {
        let ctx = VariableContext::new();

        let input = "{{missing}}/path";
        let output = VariableResolver::substitute(input, &ctx);
        // 未找到的变量保持原样
        assert_eq!(output, "{{missing}}/path");
    }

    #[test]
    fn test_resolve_env_vars() {
        // 设置测试环境变量
        unsafe {
            std::env::set_var("TEST_VAR", "test_value");
        }

        let input = "Value: ${TEST_VAR}";
        let output = VariableResolver::resolve_env_vars(input);
        assert_eq!(output, "Value: test_value");

        // 清理
        unsafe {
            std::env::remove_var("TEST_VAR");
        }
    }

    #[test]
    fn test_resolve_env_vars_missing() {
        let input = "Value: ${NONEXISTENT_VAR}";
        let output = VariableResolver::resolve_env_vars(input);
        // 未找到的环境变量保持原样
        assert_eq!(output, "Value: ${NONEXISTENT_VAR}");
    }

    #[test]
    fn test_resolve_combined() {
        unsafe {
            std::env::set_var("API_KEY", "secret-key");
        }

        let mut ctx = VariableContext::new();
        ctx.insert("host", "api.example.com");

        let input = "https://{{host}}/auth?key=${API_KEY}";
        let output = VariableResolver::resolve(input, &ctx);
        assert_eq!(output, "https://api.example.com/auth?key=secret-key");

        unsafe {
            std::env::remove_var("API_KEY");
        }
    }

    #[test]
    fn test_resolve_builtin_uuid() {
        let ctx = VariableContext::new();
        let input = "ID: {{$uuid}}";
        let output1 = VariableResolver::resolve(input, &ctx);
        let output2 = VariableResolver::resolve(input, &ctx);

        assert!(output1.starts_with("ID: "));
        assert_eq!(output1.len(), 4 + 36); // "ID: " + 36 char UUID
        // 验证两次生成的 UUID 不同
        assert_ne!(output1, output2);
    }

    #[test]
    fn test_resolve_builtin_timestamp() {
        let ctx = VariableContext::new();
        let input = "Time: {{$timestamp}}";
        let output = VariableResolver::resolve(input, &ctx);

        assert!(output.starts_with("Time: "));
        // 简单验证时间戳长度 (例如 1714543200 是 10 位)
        assert!(output.len() >= 14);
    }

    #[test]
    fn test_resolve_builtin_random_int() {
        let ctx = VariableContext::new();
        let input = "Rand: {{$random_int}}";
        let output = VariableResolver::resolve(input, &ctx);

        assert!(output.starts_with("Rand: "));
        let num_str = &output[6..];
        let num: u32 = num_str.parse().unwrap();
        assert!((1..=10000).contains(&num));
    }

    #[test]
    fn test_resolve_global() {
        let mut ctx = VariableContext::new();
        ctx.insert("global.token", "global-secret");

        let input = "Bearer {{global.token}}";
        let output = VariableResolver::resolve(input, &ctx);
        assert_eq!(output, "Bearer global-secret");
    }

    #[test]
    fn test_resolve_env_via_placeholder() {
        unsafe {
            std::env::set_var("TEST_PLACEHOLDER", "env-value");
        }

        let ctx = VariableContext::new();
        let input = "Val: {{env.TEST_PLACEHOLDER}}";
        let output = VariableResolver::resolve(input, &ctx);
        assert_eq!(output, "Val: env-value");

        unsafe {
            std::env::remove_var("TEST_PLACEHOLDER");
        }
    }

    #[test]
    fn test_resolve_parsed_request() {
        let mut ctx = VariableContext::new();
        ctx.insert("host", "example.com");
        ctx.insert("user_id", "456");
        ctx.insert("base_url", "https://api.test.com/");
        ctx.insert("user_agent", "CustomTestAgent/1.0");

        let mut parsed = crate::parser::ParsedRequest::new(1);
        parsed.url = "/users/{{user_id}}".to_string();
        parsed.base_path = Some("v1".to_string());
        parsed.headers = vec![("Host".to_string(), "{{host}}".to_string())];
        parsed.body = Some("hello {{user_id}}".to_string());

        VariableResolver::resolve_parsed_request(&mut parsed, &mut ctx);

        // 验证 URL 渲染（此处不再自动拼接，仅替换占位符）
        assert_eq!(parsed.url, "/users/456");

        // 验证 Headers 渲染和 user_agent 自动注入
        assert_eq!(parsed.headers.len(), 2);
        assert_eq!(
            parsed.headers[0],
            ("Host".to_string(), "example.com".to_string())
        );
        assert_eq!(
            parsed.headers[1],
            ("User-Agent".to_string(), "CustomTestAgent/1.0".to_string())
        );

        // 验证 Body 渲染
        assert_eq!(parsed.body.unwrap(), "hello 456");
    }
}
