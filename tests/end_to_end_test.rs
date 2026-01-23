use rupost::parser::{HttpFileParser, MarkdownFileParser};
use rupost::runner::TestExecutor;
use rupost::variable::{ConfigLoader, VariableResolver};
use std::fs;
use tempfile::TempDir;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// 测试完整 HTTP 文件解析和执行流程
#[tokio::test]
async fn test_http_file_end_to_end() {
    // 启动模拟服务器
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/users"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "users": [
                {"id": 1, "name": "Alice"},
                {"id": 2, "name": "Bob"}
            ]
        })))
        .mount(&mock_server)
        .await;

    // 创建临时 HTTP 文件
    let temp_dir = TempDir::new().unwrap();
    let http_file = temp_dir.path().join("test.http");

    let content = format!(
        r#"
### Get Users
GET {}/api/users
Accept: application/json

###
"#,
        mock_server.uri()
    );

    fs::write(&http_file, content).unwrap();

    // 解析文件
    let parsed = HttpFileParser::parse_file(&http_file).unwrap();
    assert_eq!(parsed.requests.len(), 1);

    // 执行请求
    let executor = TestExecutor::new();
    let results = executor.execute_all(parsed).await.unwrap();

    // 验证结果
    assert_eq!(results.len(), 1);
    assert!(results[0].success);
}

/// 测试完整 Markdown 文件解析和执行流程
#[tokio::test]
async fn test_markdown_file_end_to_end() {
    // 启动模拟服务器
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/login"))
        .and(header("Content-Type", "application/json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "token": "test-token-123",
            "user": {
                "id": 1,
                "email": "test@example.com"
            }
        })))
        .mount(&mock_server)
        .await;

    // 创建临时 Markdown 文件
    let temp_dir = TempDir::new().unwrap();
    let md_file = temp_dir.path().join("api-docs.md");

    let content = format!(
        r#"
# API Documentation

## Authentication

### Login

```http
POST {}/api/login
Content-Type: application/json

{{
  "email": "test@example.com",
  "password": "password123"
}}
```
"#,
        mock_server.uri()
    );

    fs::write(&md_file, content).unwrap();

    // 解析文件
    let parsed = MarkdownFileParser::parse_file(&md_file).unwrap();
    assert_eq!(parsed.requests.len(), 1);

    // 执行请求
    let executor = TestExecutor::new();
    let results = executor.execute_all(parsed).await.unwrap();

    // 验证结果
    assert_eq!(results.len(), 1);
    assert!(results[0].success);
}

/// 测试带变量替换的完整流程
#[tokio::test]
async fn test_variable_substitution_end_to_end() {
    // 启动模拟服务器
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/v1/users/123"))
        .and(header("Authorization", "Bearer test-api-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 123,
            "name": "Test User"
        })))
        .mount(&mock_server)
        .await;

    // 创建临时配置文件
    let temp_dir = TempDir::new().unwrap();
    let config_file = temp_dir.path().join("rupost.toml");

    let base_url = mock_server.uri();
    let config_content = format!(
        r#"
[environments.test]
base_url = "{}"
api_version = "v1"
api_key = "test-api-key"
user_id = "123"
"#,
        base_url
    );

    fs::write(&config_file, config_content).unwrap();

    // 创建 HTTP 文件（带变量）
    let http_file = temp_dir.path().join("test.http");
    let http_content = r#"
### Get User
GET {{base_url}}/{{api_version}}/users/{{user_id}}
Authorization: Bearer {{api_key}}

###
"#;

    fs::write(&http_file, http_content).unwrap();

    // 加载配置并构建变量上下文
    let config = ConfigLoader::load_from_path(&config_file).unwrap();
    let context = ConfigLoader::build_context(&config, Some("test"), &[]);

    // 解析文件
    let mut parsed = HttpFileParser::parse_file(&http_file).unwrap();

    // 应用变量替换
    for request in &mut parsed.requests {
        request.url = VariableResolver::substitute(&request.url, &context);
        // headers 是 Vec 不是 Option
        for (_, value) in &mut request.headers {
            *value = VariableResolver::substitute(value, &context);
        }
        if let Some(body) = &mut request.body {
            *body = VariableResolver::substitute(body, &context);
        }
    }

    // 执行请求
    let executor = TestExecutor::new();
    let results = executor.execute_all(parsed).await.unwrap();

    // 验证结果
    assert_eq!(results.len(), 1);
    assert!(results[0].success);
}

/// 测试断言系统集成
#[tokio::test]
async fn test_assertion_integration() {
    // 启动模拟服务器
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/status"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({
                    "status": "healthy",
                    "version": "1.2.3",
                    "uptime": 12345
                }))
                .insert_header("X-Request-ID", "req-123"),
        )
        .mount(&mock_server)
        .await;

    // 创建临时 HTTP 文件（带断言）
    let temp_dir = TempDir::new().unwrap();
    let http_file = temp_dir.path().join("test.http");

    let content = format!(
        r#"
### Check Status
# @assert status == 200
# @assert body.status == "healthy"
# @assert header.X-Request-ID exists
# @assert body.uptime > 10000
GET {}/api/status

###
"#,
        mock_server.uri()
    );

    fs::write(&http_file, content).unwrap();

    // 解析文件
    let parsed = HttpFileParser::parse_file(&http_file).unwrap();
    assert_eq!(parsed.requests.len(), 1);

    // 打印调试信息
    println!("断言数量: {}", parsed.requests[0].metadata.assertions.len());
    for (i, assertion) in parsed.requests[0].metadata.assertions.iter().enumerate() {
        println!("  断言 {}: {}", i + 1, assertion);
    }

    // 执行请求
    let executor = TestExecutor::new();
    let results = executor.execute_all(parsed).await.unwrap();

    // 验证结果
    assert_eq!(results.len(), 1);
    assert!(results[0].success);

    // 如果有断言，验证它们都通过
    if !results[0].assertions.is_empty() {
        assert!(results[0].assertions.iter().all(|a| a.passed));
    }
}

/// 测试多请求链式执行
#[tokio::test]
async fn test_multi_request_chain() {
    // 启动模拟服务器
    let mock_server = MockServer::start().await;

    // 登录接口
    Mock::given(method("POST"))
        .and(path("/api/login"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "token": "auth-token-456"
        })))
        .mount(&mock_server)
        .await;

    // 用户信息接口
    Mock::given(method("GET"))
        .and(path("/api/profile"))
        .and(header("Authorization", "Bearer auth-token-456"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 1,
            "name": "John Doe"
        })))
        .mount(&mock_server)
        .await;

    // 创建临时 HTTP 文件
    let temp_dir = TempDir::new().unwrap();
    let http_file = temp_dir.path().join("test.http");

    let content = format!(
        r#"
### Login
POST {}/api/login
Content-Type: application/json

{{
  "username": "john",
  "password": "secret"
}}

### Get Profile
GET {}/api/profile
Authorization: Bearer auth-token-456

###
"#,
        mock_server.uri(),
        mock_server.uri()
    );

    fs::write(&http_file, content).unwrap();

    // 解析文件
    let parsed = HttpFileParser::parse_file(&http_file).unwrap();
    assert_eq!(parsed.requests.len(), 2);

    // 执行请求
    let executor = TestExecutor::new();
    let results = executor.execute_all(parsed).await.unwrap();

    // 验证结果
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.success));
}

/// 测试环境变量集成
#[tokio::test]
async fn test_system_env_var_integration() {
    unsafe {
        std::env::set_var("TEST_API_KEY", "env-api-key-789");
    }

    // 启动模拟服务器
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/data"))
        .and(header("X-API-Key", "env-api-key-789"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": "success"
        })))
        .mount(&mock_server)
        .await;

    // 创建临时配置文件（引用环境变量）
    let temp_dir = TempDir::new().unwrap();
    let config_file = temp_dir.path().join("rupost.toml");

    let config_content = format!(
        r#"
[environments.test]
base_url = "{}"
api_key = "${{TEST_API_KEY}}"
"#,
        mock_server.uri()
    );

    fs::write(&config_file, config_content).unwrap();

    // 创建 HTTP 文件
    let http_file = temp_dir.path().join("test.http");
    let http_content = r#"
### Test API
GET {{base_url}}/api/data
X-API-Key: {{api_key}}

###
"#;

    fs::write(&http_file, http_content).unwrap();

    // 加载配置并构建变量上下文
    let config = ConfigLoader::load_from_path(&config_file).unwrap();
    let context = ConfigLoader::build_context(&config, Some("test"), &[]);

    // 解析文件
    let mut parsed = HttpFileParser::parse_file(&http_file).unwrap();

    // 应用变量替换
    for request in &mut parsed.requests {
        request.url = VariableResolver::substitute(&request.url, &context);
        for (_, value) in &mut request.headers {
            *value = VariableResolver::substitute(value, &context);
        }
    }

    // 执行请求
    let executor = TestExecutor::new();
    let results = executor.execute_all(parsed).await.unwrap();

    // 验证结果
    assert_eq!(results.len(), 1);
    assert!(results[0].success);

    unsafe {
        std::env::remove_var("TEST_API_KEY");
    }
}

/// 测试 CLI 变量覆盖的完整流程
#[tokio::test]
async fn test_cli_override_end_to_end() {
    // 启动模拟服务器
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/items"))
        .and(header("X-Custom-Header", "cli-override-value"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "items": []
        })))
        .mount(&mock_server)
        .await;

    // 创建临时配置文件
    let temp_dir = TempDir::new().unwrap();
    let config_file = temp_dir.path().join("rupost.toml");

    let config_content = format!(
        r#"
[environments.test]
base_url = "{}"
custom_header = "config-value"
"#,
        mock_server.uri()
    );

    fs::write(&config_file, config_content).unwrap();

    // 创建 HTTP 文件
    let http_file = temp_dir.path().join("test.http");
    let http_content = r#"
### Test API
GET {{base_url}}/api/items
X-Custom-Header: {{custom_header}}

###
"#;

    fs::write(&http_file, http_content).unwrap();

    // 使用 CLI 变量覆盖
    let cli_vars = vec![(
        "custom_header".to_string(),
        "cli-override-value".to_string(),
    )];
    let config = ConfigLoader::load_from_path(&config_file).unwrap();
    let context = ConfigLoader::build_context(&config, Some("test"), &cli_vars);

    // 解析文件
    let mut parsed = HttpFileParser::parse_file(&http_file).unwrap();

    // 应用变量替换
    for request in &mut parsed.requests {
        request.url = VariableResolver::substitute(&request.url, &context);
        for (_, value) in &mut request.headers {
            *value = VariableResolver::substitute(value, &context);
        }
    }

    // 执行请求
    let executor = TestExecutor::new();
    let results = executor.execute_all(parsed).await.unwrap();

    // 验证结果
    assert_eq!(results.len(), 1);
    assert!(results[0].success);
}
