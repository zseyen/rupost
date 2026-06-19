use rupost::template::run_template;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_template_generation_http() {
    let temp_dir = TempDir::new().unwrap();
    let http_path = temp_dir.path().join("test_sse.http");
    let env_path = temp_dir.path().join(".env.example");

    // 1. 生成 .http 模板
    let res = run_template("sse", Some(http_path.to_str().unwrap()), false, false);
    assert!(res.is_ok());

    // 2. 检查文件是否存在及内容
    assert!(http_path.exists());
    assert!(env_path.exists());

    let http_content = fs::read_to_string(&http_path).unwrap();
    assert!(http_content.contains("# RuPost SSE & 大模型 API 测试模板"));
    assert!(http_content.contains("POST http://127.0.0.1:8080"));

    let env_content = fs::read_to_string(&env_path).unwrap();
    assert!(env_content.contains("API_KEY="));
}

#[test]
fn test_template_generation_markdown() {
    let temp_dir = TempDir::new().unwrap();
    let md_path = temp_dir.path().join("test_sse.md");
    let env_path = temp_dir.path().join(".env.example");

    // 1. 生成 .md 模板
    let res = run_template("sse", Some(md_path.to_str().unwrap()), false, false);
    assert!(res.is_ok());

    // 2. 检查文件是否存在及内容
    assert!(md_path.exists());
    assert!(env_path.exists());

    let md_content = fs::read_to_string(&md_path).unwrap();
    assert!(md_content.contains("# RuPost SSE & 大模型 API 测试模板 (Markdown 格式)"));
    assert!(md_content.contains("```http"));

    let env_content = fs::read_to_string(&env_path).unwrap();
    assert!(env_content.contains("API_KEY="));
}

#[test]
fn test_template_safety_overwrite_protection() {
    let temp_dir = TempDir::new().unwrap();
    let http_path = temp_dir.path().join("existing.http");
    let env_path = temp_dir.path().join(".env.example");

    // 1. 创建空文件模拟已存在
    fs::write(&http_path, "original user content").unwrap();

    // 2. 尝试不带 --force 生成，预期失败
    let res = run_template("sse", Some(http_path.to_str().unwrap()), false, false);
    assert!(res.is_err());
    assert!(res.unwrap_err().to_string().contains("already exists"));

    // 3. 验证原内容未被覆盖，且没有生成 .env.example
    assert_eq!(
        fs::read_to_string(&http_path).unwrap(),
        "original user content"
    );
    assert!(!env_path.exists());

    // 4. 尝试带 --force 生成，预期成功
    let res_force = run_template("sse", Some(http_path.to_str().unwrap()), true, false);
    assert!(res_force.is_ok());

    // 5. 验证内容被成功覆盖，并且伴随生成了 .env.example
    let new_content = fs::read_to_string(&http_path).unwrap();
    assert!(new_content.contains("# RuPost SSE"));
    assert!(env_path.exists());
}

#[test]
fn test_template_safety_overwrite_protection_env() {
    let temp_dir = TempDir::new().unwrap();
    let http_path = temp_dir.path().join("existing.http");
    let env_path = temp_dir.path().join(".env.example");

    // 1. 模拟 .env.example 已存在，而 http 文件不存在
    fs::write(&env_path, "original env config").unwrap();

    // 2. 尝试不带 --force 生成，预期失败，因为 .env.example 冲突
    let res = run_template("sse", Some(http_path.to_str().unwrap()), false, false);
    assert!(res.is_err());
    assert!(res.unwrap_err().to_string().contains("already exists"));

    // 3. 验证两个文件均处于保护状态，未被生成/覆盖
    assert!(!http_path.exists());
    assert_eq!(
        fs::read_to_string(&env_path).unwrap(),
        "original env config"
    );
}

#[test]
fn test_template_auto_create_parent_dir() {
    let temp_dir = TempDir::new().unwrap();
    let nested_http_path = temp_dir.path().join("nested_a/nested_b/template.http");
    let nested_env_path = temp_dir.path().join("nested_a/nested_b/.env.example");

    // 1. 自动生成多级目录模板
    let res = run_template(
        "sse",
        Some(nested_http_path.to_str().unwrap()),
        false,
        false,
    );
    assert!(res.is_ok());

    // 2. 验证多层父级目录及文件是否被正确建立
    assert!(nested_http_path.exists());
    assert!(nested_env_path.exists());
}

#[test]
fn test_template_unsupported_type() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("out.http");

    // 1. 传入无效模板名，预期失败
    let res = run_template(
        "invalid_template_type",
        Some(path.to_str().unwrap()),
        false,
        false,
    );
    assert!(res.is_err());
    assert!(
        res.unwrap_err()
            .to_string()
            .contains("Unsupported template type")
    );

    // 2. 验证未生成任何文件
    assert!(!path.exists());
}

#[test]
fn test_template_list_command() {
    let temp_dir = TempDir::new().unwrap();
    let path = temp_dir.path().join("out.http");

    // 1. 触发 list，预期直接返回 ok，且不在指定路径生成任何文件
    let res = run_template("sse", Some(path.to_str().unwrap()), false, true);
    assert!(res.is_ok());
    assert!(!path.exists());
}

#[test]
fn test_default_output_path_resolution() {
    let temp_dir = TempDir::new().unwrap();
    let old_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(temp_dir.path()).unwrap();

    let res = run_template("config", None, false, false);
    assert!(res.is_ok());
    assert!(temp_dir.path().join("rupost.toml").exists());

    let res_sse = run_template("sse", None, false, false);
    assert!(res_sse.is_ok());
    assert!(temp_dir.path().join("sse_template.http").exists());

    std::env::set_current_dir(old_dir).unwrap();
}

#[test]
fn test_config_overwrite_protection() {
    let temp_dir = TempDir::new().unwrap();
    let toml_path = temp_dir.path().join("rupost.toml");
    fs::write(&toml_path, "user config").unwrap();

    // 1. 不带 force，应当报错
    let res = run_template("config", Some(toml_path.to_str().unwrap()), false, false);
    assert!(res.is_err());
    assert_eq!(fs::read_to_string(&toml_path).unwrap(), "user config");

    // 2. 带 force，应当成功覆盖，并发出警告提醒 (只提醒不终止)
    let res_force = run_template("config", Some(toml_path.to_str().unwrap()), true, false);
    assert!(res_force.is_ok());
    assert!(
        fs::read_to_string(&toml_path)
            .unwrap()
            .contains("environments.dev")
    );
}
