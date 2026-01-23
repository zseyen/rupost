use crate::variable::types::VariableContext;
use regex::{Captures, Regex};
use std::sync::OnceLock;

/// 变量替换器
pub struct VariableResolver;

impl VariableResolver {
    /// 替换文本中的所有 {{variable}} 占位符
    pub fn substitute(text: &str, context: &VariableContext) -> String {
        static VAR_REGEX: OnceLock<Regex> = OnceLock::new();
        let re = VAR_REGEX.get_or_init(|| Regex::new(r"\{\{([a-zA-Z_][a-zA-Z0-9_]*)\}\}").unwrap());

        re.replace_all(text, |caps: &Captures| {
            let var_name = &caps[1];
            context.get(var_name).unwrap_or(&caps[0]).to_string()
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
}
