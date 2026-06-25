use rupost::parser::HttpFileParser;
use rupost::runner::TestExecutor;
use rupost::variable::VariableContext;

#[tokio::test]
async fn test_auto_routing_and_stitching() {
    let mock_server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/v1/chat/completions"))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": "hello"}}]
            })),
        )
        .mount(&mock_server)
        .await;

    let mock_uri = mock_server.uri();
    let mut context = VariableContext::new();
    context.insert("base_url".to_string(), mock_uri.clone());

    let http_content = r#"
# @base_path /v1/chat/completions
@name test-post
POST /
Content-Type: application/json

{"hello": "world"}
"#;
    let parsed_file = HttpFileParser::parse_content(http_content).unwrap();
    let executor = TestExecutor::new();

    let results = executor
        .execute_all(parsed_file, &mut context)
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].success, "Error: {:?}", results[0].error);
    assert_eq!(results[0].url, format!("{}/v1/chat/completions", mock_uri));
}

#[tokio::test]
async fn test_auto_routing_no_base_path() {
    let mock_server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/v1/users"))
        .respond_with(wiremock::ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    let mock_uri = mock_server.uri();
    let mut context = VariableContext::new();
    context.insert("base_url".to_string(), format!("{}/v1", mock_uri));

    let http_content = r#"
GET /users
"#;
    let parsed_file = HttpFileParser::parse_content(http_content).unwrap();
    let executor = TestExecutor::new();
    let results = executor
        .execute_all(parsed_file, &mut context)
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].success);
    assert_eq!(results[0].url, format!("{}/v1/users", mock_uri));
}
