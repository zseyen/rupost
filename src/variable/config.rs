use crate::variable::resolver::VariableResolver;
use crate::variable::types::{VariableConfig, VariableContext};
use std::fs;
use std::path::Path;

/// 配置文件加载器
pub struct ConfigLoader;

impl ConfigLoader {
    /// 配置文件名
    const CONFIG_FILE: &'static str = "rupost.toml";

    /// 从指定路径加载配置文件
    pub fn load_from_path<P: AsRef<Path>>(path: P) -> Result<VariableConfig, String> {
        let content = fs::read_to_string(path.as_ref())
            .map_err(|e| format!("Failed to read config file: {}", e))?;

        toml::from_str(&content).map_err(|e| format!("Failed to parse config file: {}", e))
    }

    /// 查找并加载配置文件
    /// 查找顺序：
    /// 1. 当前目录
    /// 2. 父目录递归查找
    /// 3. 用户配置目录 ~/.config/rupost/
    pub fn find_and_load() -> Option<VariableConfig> {
        // 1. 当前目录
        if let Some(config) = Self::try_load_from_current_dir() {
            return Some(config);
        }

        // 2. 用户配置目录
        if let Some(config) = Self::try_load_from_user_dir() {
            return Some(config);
        }

        None
    }

    /// 尝试从当前目录及其父目录加载
    fn try_load_from_current_dir() -> Option<VariableConfig> {
        let mut current = std::env::current_dir().ok()?;

        loop {
            let config_path = current.join(Self::CONFIG_FILE);
            if config_path.exists() {
                return Self::load_from_path(&config_path).ok();
            }

            // 尝试父目录
            if !current.pop() {
                break;
            }
        }

        None
    }

    /// 尝试从用户配置目录加载
    fn try_load_from_user_dir() -> Option<VariableConfig> {
        let home = dirs::home_dir()?;
        let config_path = home.join(".config").join("rupost").join(Self::CONFIG_FILE);

        if config_path.exists() {
            Self::load_from_path(&config_path).ok()
        } else {
            None
        }
    }

    /// 构建变量上下文
    /// env_name: 环境名称（如 "dev", "prod"）
    /// cli_vars: CLI 传入的变量覆盖（--var key=value）
    pub fn build_context(
        config: &VariableConfig,
        env_name: Option<&str>,
        cli_vars: &[(String, String)],
    ) -> VariableContext {
        let mut context = VariableContext::new();

        // 1. 从配置文件加载环境变量
        if let Some(env) = env_name.and_then(|name| config.get_environment(name)) {
            for (key, value) in &env.variables {
                // 解析系统环境变量 ${VAR}
                let resolved_value = VariableResolver::resolve_env_vars(value);
                context.insert(key.clone(), resolved_value);
            }
        }

        // 2. 应用 CLI 覆盖（优先级最高）
        for (key, value) in cli_vars {
            context.insert(key.clone(), value.clone());
        }

        context
    }

    /// 解析 CLI 变量参数 "key=value"
    pub fn parse_cli_var(s: &str) -> Option<(String, String)> {
        s.split_once('=')
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_load_from_path() {
        let config_content = r#"
[environments.dev]
base_url = "http://localhost:8080"
api_key = "dev-key"
"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(config_content.as_bytes()).unwrap();
        temp_file.flush().unwrap();

        let config = ConfigLoader::load_from_path(temp_file.path()).unwrap();
        assert_eq!(config.environments.len(), 1);
        assert!(config.get_environment("dev").is_some());
    }

    #[test]
    fn test_build_context() {
        let config_content = r#"
[environments.dev]
base_url = "http://localhost:8080"
token = "dev-token"

[environments.prod]
base_url = "https://api.example.com"
token = "${PROD_TOKEN}"
"#;

        let config: VariableConfig = toml::from_str(config_content).unwrap();

        // 测试 dev 环境
        let context = ConfigLoader::build_context(&config, Some("dev"), &[]);
        assert_eq!(context.get("base_url"), Some("http://localhost:8080"));
        assert_eq!(context.get("token"), Some("dev-token"));

        // 测试 CLI 覆盖
        let cli_vars = vec![("token".to_string(), "custom-token".to_string())];
        let context = ConfigLoader::build_context(&config, Some("dev"), &cli_vars);
        assert_eq!(context.get("token"), Some("custom-token"));
    }

    #[test]
    fn test_parse_cli_var() {
        assert_eq!(
            ConfigLoader::parse_cli_var("key=value"),
            Some(("key".to_string(), "value".to_string()))
        );

        assert_eq!(
            ConfigLoader::parse_cli_var("url=https://example.com"),
            Some(("url".to_string(), "https://example.com".to_string()))
        );

        assert_eq!(ConfigLoader::parse_cli_var("invalid"), None);
    }
}
