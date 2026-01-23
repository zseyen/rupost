use rupost::variable::{ConfigLoader, VariableContext, VariableResolver};
use std::fs;
use tempfile::TempDir;

/// 测试从实际配置文件加载变量
#[test]
fn test_load_config_from_file() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("rupost.toml");

    let config_content = r#"
[environments.dev]
base_url = "http://localhost:3000"
api_key = "dev-key-123"

[environments.prod]
base_url = "https://api.example.com"
api_key = "${PROD_API_KEY}"
"#;

    fs::write(&config_path, config_content).unwrap();

    let config = ConfigLoader::load_from_path(&config_path).unwrap();
    assert!(config.environments.contains_key("dev"));
    assert!(config.environments.contains_key("prod"));

    let dev_env = &config.environments["dev"];
    assert_eq!(
        dev_env.variables.get("base_url"),
        Some(&"http://localhost:3000".to_string())
    );
    assert_eq!(
        dev_env.variables.get("api_key"),
        Some(&"dev-key-123".to_string())
    );
}

/// 测试变量上下文构建
#[test]
fn test_build_context_with_environment() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("rupost.toml");

    let config_content = r#"
[environments.dev]
base_url = "http://localhost:3000"
timeout = "30"

[environments.staging]
base_url = "http://staging.example.com"
timeout = "60"
"#;

    fs::write(&config_path, config_content).unwrap();

    // 加载配置
    let config = ConfigLoader::load_from_path(&config_path).unwrap();

    // 构建 dev 环境上下文
    let context = ConfigLoader::build_context(&config, Some("dev"), &[]);
    assert_eq!(context.get("base_url"), Some("http://localhost:3000"));
    assert_eq!(context.get("timeout"), Some("30"));

    // 构建 staging 环境上下文
    let context = ConfigLoader::build_context(&config, Some("staging"), &[]);
    assert_eq!(context.get("base_url"), Some("http://staging.example.com"));
    assert_eq!(context.get("timeout"), Some("60"));
}

/// 测试 CLI 变量覆盖优先级
#[test]
fn test_cli_override_priority() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("rupost.toml");

    let config_content = r#"
[environments.dev]
base_url = "http://localhost:3000"
api_key = "config-key"
"#;

    fs::write(&config_path, config_content).unwrap();

    // 加载配置
    let config = ConfigLoader::load_from_path(&config_path).unwrap();

    // 使用 CLI 变量覆盖
    let cli_vars = vec![("api_key".to_string(), "cli-override-key".to_string())];
    let context = ConfigLoader::build_context(&config, Some("dev"), &cli_vars);

    // CLI 变量应该覆盖配置文件中的值
    assert_eq!(context.get("api_key"), Some("cli-override-key"));
    assert_eq!(context.get("base_url"), Some("http://localhost:3000"));
}

/// 测试环境变量解析
#[test]
fn test_environment_variable_resolution() {
    unsafe {
        std::env::set_var("TEST_ENV_VAR", "environment-value");
    }

    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("rupost.toml");

    let config_content = r#"
[environments.dev]
base_url = "http://localhost:3000"
api_key = "${TEST_ENV_VAR}"
"#;

    fs::write(&config_path, config_content).unwrap();

    // 加载配置
    let config = ConfigLoader::load_from_path(&config_path).unwrap();
    let context = ConfigLoader::build_context(&config, Some("dev"), &[]);

    let result = VariableResolver::substitute("{{api_key}}", &context);
    assert_eq!(result, "environment-value");

    unsafe {
        std::env::remove_var("TEST_ENV_VAR");
    }
}

/// 测试复杂变量替换场景
#[test]
fn test_complex_variable_substitution() {
    let mut context = VariableContext::new();
    context.insert("protocol", "https");
    context.insert("host", "api.example.com");
    context.insert("version", "v1");
    context.insert("resource", "users");

    // 测试 URL 组合
    let url =
        VariableResolver::substitute("{{protocol}}://{{host}}/{{version}}/{{resource}}", &context);
    assert_eq!(url, "https://api.example.com/v1/users");

    // 测试请求头（缺失变量保持原样）
    let auth_header = VariableResolver::substitute("Bearer {{api_key}}", &context);
    assert_eq!(auth_header, "Bearer {{api_key}}");

    // 测试请求体
    context.insert("user_id", "12345");
    context.insert("username", "test_user");

    let body =
        VariableResolver::substitute(r#"{"id": "{{user_id}}", "name": "{{username}}"}"#, &context);
    assert_eq!(body, r#"{"id": "12345", "name": "test_user"}"#);
}

/// 测试多环境切换
#[test]
fn test_multi_environment_switching() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("rupost.toml");

    let config_content = r#"
[environments.dev]
base_url = "http://localhost:3000"
db_name = "dev_db"

[environments.test]
base_url = "http://test-server:3000"
db_name = "test_db"

[environments.prod]
base_url = "https://api.example.com"
db_name = "prod_db"
"#;

    fs::write(&config_path, config_content).unwrap();

    // 加载配置
    let config = ConfigLoader::load_from_path(&config_path).unwrap();

    // 测试 dev 环境
    let context = ConfigLoader::build_context(&config, Some("dev"), &[]);
    assert_eq!(
        VariableResolver::substitute("{{base_url}}", &context),
        "http://localhost:3000"
    );
    assert_eq!(
        VariableResolver::substitute("{{db_name}}", &context),
        "dev_db"
    );

    // 测试 test 环境
    let context = ConfigLoader::build_context(&config, Some("test"), &[]);
    assert_eq!(
        VariableResolver::substitute("{{base_url}}", &context),
        "http://test-server:3000"
    );
    assert_eq!(
        VariableResolver::substitute("{{db_name}}", &context),
        "test_db"
    );

    // 测试 prod 环境
    let context = ConfigLoader::build_context(&config, Some("prod"), &[]);
    assert_eq!(
        VariableResolver::substitute("{{base_url}}", &context),
        "https://api.example.com"
    );
    assert_eq!(
        VariableResolver::substitute("{{db_name}}", &context),
        "prod_db"
    );
}

/// 测试默认环境（无环境指定时不加载任何环境变量）
#[test]
fn test_default_environment() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("rupost.toml");

    let config_content = r#"
[environments.dev]
base_url = "http://localhost:3000"
"#;

    fs::write(&config_path, config_content).unwrap();

    // 加载配置
    let config = ConfigLoader::load_from_path(&config_path).unwrap();

    // 不指定环境时，上下文为空
    let context = ConfigLoader::build_context(&config, None, &[]);
    assert!(context.is_empty());
}

/// 测试缺失变量的处理
#[test]
fn test_missing_variable_handling() {
    let context = VariableContext::new();

    // 缺失的变量应该保持原样
    let result = VariableResolver::substitute("{{missing_var}}", &context);
    assert_eq!(result, "{{missing_var}}");

    // 混合已定义和未定义的变量
    let mut context = VariableContext::new();
    context.insert("defined", "value");

    let result = VariableResolver::substitute("{{defined}} and {{undefined}}", &context);
    assert_eq!(result, "value and {{undefined}}");
}

/// 测试空配置文件
#[test]
fn test_empty_config() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("rupost.toml");

    fs::write(&config_path, "").unwrap();

    let result = ConfigLoader::load_from_path(&config_path);
    assert!(result.is_ok());
    let config = result.unwrap();
    assert!(config.environments.is_empty());
}

/// 测试 CLI 变量解析格式
#[test]
fn test_cli_variable_parsing() {
    let cli_var1 = "key1=value1";
    let cli_var2 = "key2=value with spaces";
    let cli_var3 = "key3=value=with=equals";

    let (k1, v1) = ConfigLoader::parse_cli_var(cli_var1).unwrap();
    assert_eq!(k1, "key1");
    assert_eq!(v1, "value1");

    let (k2, v2) = ConfigLoader::parse_cli_var(cli_var2).unwrap();
    assert_eq!(k2, "key2");
    assert_eq!(v2, "value with spaces");

    let (k3, v3) = ConfigLoader::parse_cli_var(cli_var3).unwrap();
    assert_eq!(k3, "key3");
    assert_eq!(v3, "value=with=equals");
}
