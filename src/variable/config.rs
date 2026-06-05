use crate::variable::resolver::VariableResolver;
use crate::variable::types::{VariableConfig, VariableContext};
use std::collections::HashMap;
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
    /// env_file: 指定读取的局部环境变量文件路径（如 ".env"），为 None 时不加载任何 `.env` 文件
    pub fn build_context(
        config: &VariableConfig,
        env_name: Option<&str>,
        cli_vars: &[(String, String)],
        env_file: Option<&str>,
    ) -> VariableContext {
        let mut context = VariableContext::new();
        let mut vars = HashMap::new();

        // 1. 从共享 toml 配置文件加载环境变量
        if let Some(env) = env_name.and_then(|name| config.get_environment(name)) {
            for (key, value) in &env.variables {
                // 解析系统环境变量 ${VAR}
                let resolved_value = VariableResolver::resolve_env_vars(value);
                vars.insert(key.clone(), resolved_value);
            }
        }

        // 2. 自动检测并加载本地局部 `.env` 文件 (覆盖 toml)
        if let Some(env_vars) =
            env_file.and_then(|p| crate::variable::env_file::EnvFileParser::parse_file(p).ok())
        {
            for (key, value) in env_vars {
                vars.insert(key, value);
            }
        }

        // 如果指定了具体环境名称，如 prod，也可以尝试加载特定环境的本地文件 `.env.prod`
        if let Some(name) = env_name {
            let env_file_name = format!(".env.{}", name);
            if let Ok(env_vars) =
                crate::variable::env_file::EnvFileParser::parse_file(&env_file_name)
            {
                for (key, value) in env_vars {
                    vars.insert(key, value);
                }
            }
        }

        // 3. 应用当前进程的系统环境变量进行同名覆盖 (覆盖 .env & toml)
        for key in vars.keys().cloned().collect::<Vec<String>>() {
            if let Ok(sys_val) = std::env::var(&key) {
                vars.insert(key, sys_val);
            }
        }

        // 4. 应用 CLI 覆盖（优先级最高，覆盖一切）
        for (key, value) in cli_vars {
            vars.insert(key.clone(), value.clone());
        }

        context.extend(vars);
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
        let context = ConfigLoader::build_context(&config, Some("dev"), &[], None);
        assert_eq!(
            context.get("base_url").as_deref(),
            Some("http://localhost:8080")
        );
        assert_eq!(context.get("token").as_deref(), Some("dev-token"));

        // 测试 CLI 覆盖
        let cli_vars = vec![("token".to_string(), "custom-token".to_string())];
        let context = ConfigLoader::build_context(&config, Some("dev"), &cli_vars, None);
        assert_eq!(context.get("token").as_deref(), Some("custom-token"));
    }

    #[test]
    fn test_build_context_cascading_priority() {
        let config_content = r#"
[environments.dev]
base_url = "http://dev-gateway.internal:8080"
api_key = "global-key"
"#;
        let config: VariableConfig = toml::from_str(config_content).unwrap();

        // 1. 使用 tempfile 创建隔离的临时 `.env` 文件
        let env_content = r#"
base_url = http://localhost:8081
api_key = local-key
sys_var = local-sys
"#;
        let mut temp_file = tempfile::NamedTempFile::new().unwrap();
        temp_file.write_all(env_content.as_bytes()).unwrap();
        temp_file.flush().unwrap();
        let temp_path = temp_file.path().to_string_lossy().to_string();

        // 2. 注入系统环境变量（测试其覆盖 .env）
        unsafe {
            std::env::set_var("sys_var", "system-override-val");
        }

        // 3. 定义 CLI 覆盖
        let cli_vars = vec![("api_key".to_string(), "cli-override-key".to_string())];

        // 4. 执行 build_context 并验证 (传入 Some(&temp_path))
        let context =
            ConfigLoader::build_context(&config, Some("dev"), &cli_vars, Some(&temp_path));

        // 5. 清理临时系统环境变量
        unsafe {
            std::env::remove_var("sys_var");
        }

        // 6. 验证级联优先级：
        // CLI > 系统变量 > .env > toml
        assert_eq!(context.get("api_key"), Some("cli-override-key")); // CLI 覆盖了 toml 与 .env
        assert_eq!(context.get("sys_var"), Some("system-override-val")); // 系统变量覆盖了 .env
        assert_eq!(context.get("base_url"), Some("http://localhost:8081")); // .env 覆盖了 toml 中的 base_url
    }

    #[test]
    fn test_build_context_with_specific_env_file() {
        let config_content = r#"
[environments.prod]
base_url = "https://api.production.com"
api_key = "prod-global-key"
"#;
        let config: VariableConfig = toml::from_str(config_content).unwrap();

        // 创建临时 .env.prod 文件
        let env_content = "base_url = https://api.local-prod-proxy.com";
        fs::write(".env.prod", env_content).unwrap();

        // 执行加载 (env_name = "prod")
        let context = ConfigLoader::build_context(&config, Some("prod"), &[], None);

        // 清理临时文件
        let _ = fs::remove_file(".env.prod");

        // 验证特定环境文件级联覆盖成功
        assert_eq!(context.get("base_url"), Some("https://api.local-prod-proxy.com"));
        assert_eq!(context.get("api_key"), Some("prod-global-key")); // 未在 .env.prod 覆盖的依然保持 toml 中定义
    }

    #[test]
    fn test_build_context_missing_env_file_tolerance() {
        let config_content = r#"
[environments.dev]
base_url = "http://localhost:8080"
"#;
        let config: VariableConfig = toml::from_str(config_content).unwrap();

        // 传入一个不存在的 env_file，验证其优雅容错，不发生 panic 且返回默认配置
        let context = ConfigLoader::build_context(
            &config,
            Some("dev"),
            &[],
            Some("non_existent_file_path_123.env"),
        );

        assert_eq!(context.get("base_url"), Some("http://localhost:8080"));
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
