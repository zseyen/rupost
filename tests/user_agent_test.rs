//! Integration tests for dynamic User-Agent configuration and overrides.

use rupost::http::Client;
use rupost::http::request::Request;
use rupost::runner::TestExecutor;
use rupost::variable::VariableContext;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_default_user_agent() {
    let mock_server = MockServer::start().await;

    // 预期接收到默认的 User-Agent
    Mock::given(method("GET"))
        .and(path("/ua"))
        .and(header("user-agent", "rupost/1.0.0"))
        .respond_with(ResponseTemplate::new(200).set_body_string("Default UA OK"))
        .mount(&mock_server)
        .await;

    // 传入 None 时，使用兜底 UA
    let client = Client::new(None);
    let request = Request::new("GET", &format!("{}/ua", mock_server.uri())).unwrap();
    let response = client.execute(request).await.unwrap();

    assert_eq!(response.status.code(), 200);
    assert_eq!(response.body, "Default UA OK");
}

#[tokio::test]
async fn test_custom_user_agent() {
    let mock_server = MockServer::start().await;

    // 预期接收到自定义的 User-Agent
    Mock::given(method("GET"))
        .and(path("/ua"))
        .and(header("user-agent", "CustomTester/2.0.0"))
        .respond_with(ResponseTemplate::new(200).set_body_string("Custom UA OK"))
        .mount(&mock_server)
        .await;

    // 传入自定义值
    let client = Client::new(Some("CustomTester/2.0.0"));
    let request = Request::new("GET", &format!("{}/ua", mock_server.uri())).unwrap();
    let response = client.execute(request).await.unwrap();

    assert_eq!(response.status.code(), 200);
    assert_eq!(response.body, "Custom UA OK");
}

#[tokio::test]
async fn test_request_level_user_agent_override() {
    let mock_server = MockServer::start().await;

    // 预期接收到单请求级别的覆盖值
    Mock::given(method("GET"))
        .and(path("/ua"))
        .and(header("user-agent", "OverrideUA/9.9"))
        .respond_with(ResponseTemplate::new(200).set_body_string("Override UA OK"))
        .mount(&mock_server)
        .await;

    // 即使 Client 全局设置为 CustomTester/2.0.0
    let client = Client::new(Some("CustomTester/2.0.0"));
    
    let mut request = Request::new("GET", &format!("{}/ua", mock_server.uri())).unwrap();
    // 手工在单请求中加入 User-Agent
    request = request.with_header("User-Agent", "OverrideUA/9.9");
    
    let response = client.execute(request).await.unwrap();

    assert_eq!(response.status.code(), 200);
    assert_eq!(response.body, "Override UA OK");
}

#[tokio::test]
async fn test_executor_variable_context_user_agent() {
    let mock_server = MockServer::start().await;

    // 预期接收到从环境变量/配置上下文里解析出来的 User-Agent
    Mock::given(method("GET"))
        .and(path("/ua"))
        .and(header("user-agent", "EnvConfigUA/5.0"))
        .respond_with(ResponseTemplate::new(200).set_body_string("Config UA OK"))
        .mount(&mock_server)
        .await;

    // 在 VariableContext 里设置全局变量 user_agent
    let mut context = VariableContext::new();
    context.insert("user_agent", "EnvConfigUA/5.0");

    // 创建 executor，这里初始化为默认行为
    let executor = TestExecutor::new();
    
    // 构造测试 ParsedRequest
    let mut parsed = rupost::parser::ParsedRequest::new(1);
    parsed.url = format!("{}/ua", mock_server.uri());
    parsed.method = Some("GET".to_string());

    // 模拟执行，验证环境变量能够成功被 Executor 动态配置到 Client 中并发送
    let result = executor.execute_one(parsed, 1, &mut context, None).await;

    assert!(result.success);
    let resp = result.response.unwrap();
    assert_eq!(resp.body, "Config UA OK");
}
