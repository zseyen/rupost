use serde::Deserialize;
use std::collections::HashMap;

/// 变量上下文，存储所有可用变量
#[derive(Debug, Clone, Default)]
pub struct VariableContext {
    /// 变量映射表
    variables: HashMap<String, String>,
}

impl VariableContext {
    /// 创建新的空变量上下文
    pub fn new() -> Self {
        Self::default()
    }

    /// 插入变量
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.variables.insert(key.into(), value.into());
    }

    /// 设置变量 (insert 的别名)
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.insert(key, value);
    }

    /// 获取变量值
    pub fn get(&self, key: &str) -> Option<&str> {
        self.variables.get(key).map(|s| s.as_str())
    }

    /// 批量插入变量
    pub fn extend(&mut self, vars: HashMap<String, String>) {
        self.variables.extend(vars);
    }

    /// 变量数量
    pub fn len(&self) -> usize {
        self.variables.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.variables.is_empty()
    }
}

/// 环境配置
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Environment {
    /// 变量映射
    #[serde(flatten)]
    pub variables: HashMap<String, String>,
}

/// 完整的变量配置文件
#[derive(Debug, Clone, Deserialize, Default)]
pub struct VariableConfig {
    /// 所有环境配置
    #[serde(default)]
    pub environments: HashMap<String, Environment>,
}

impl VariableConfig {
    /// 获取指定环境的变量
    pub fn get_environment(&self, env_name: &str) -> Option<&Environment> {
        self.environments.get(env_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_variable_context_basic() {
        let mut ctx = VariableContext::new();
        assert!(ctx.is_empty());

        ctx.insert("key", "value");
        assert_eq!(ctx.len(), 1);
        assert_eq!(ctx.get("key"), Some("value"));
        assert_eq!(ctx.get("missing"), None);
    }

    #[test]
    fn test_variable_context_extend() {
        let mut ctx = VariableContext::new();
        let mut vars = HashMap::new();
        vars.insert("key1".to_string(), "value1".to_string());
        vars.insert("key2".to_string(), "value2".to_string());

        ctx.extend(vars);
        assert_eq!(ctx.len(), 2);
        assert_eq!(ctx.get("key1"), Some("value1"));
        assert_eq!(ctx.get("key2"), Some("value2"));
    }

    #[test]
    fn test_variable_config_parse() {
        let toml_str = r#"
[environments.dev]
base_url = "http://localhost:8080"
api_key = "dev-key"

[environments.prod]
base_url = "https://api.example.com"
api_key = "${PROD_KEY}"
"#;

        let config: VariableConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.environments.len(), 2);

        let dev = config.get_environment("dev").unwrap();
        assert_eq!(
            dev.variables.get("base_url"),
            Some(&"http://localhost:8080".to_string())
        );
    }
}
