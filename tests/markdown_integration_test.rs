use rupost::parser::MarkdownFileParser;
use std::path::PathBuf;

#[test]
fn test_parse_simple_api() {
    let path = PathBuf::from("examples/simple-api.md");
    let parsed = MarkdownFileParser::parse_file(&path).unwrap();

    // 应该有 5 个请求（GET, POST, PUT, GET搜索, DELETE）
    assert_eq!(parsed.requests.len(), 5);

    // 检查第一个请求的名称（从标题提取）
    assert_eq!(
        parsed.requests[0].metadata.name,
        Some("获取用户信息".to_string())
    );

    // 检查第二个请求
    assert_eq!(
        parsed.requests[1].metadata.name,
        Some("创建新用户".to_string())
    );

    // 验证请求方法和 URL
    assert_eq!(parsed.requests[0].method_or_default(), "GET");
    assert!(parsed.requests[0].url.contains("httpbingo.org/get"));
}

#[test]
fn test_parse_auth_examples() {
    let path = PathBuf::from("examples/auth-examples.md");
    let parsed = MarkdownFileParser::parse_file(&path).unwrap();

    // 应该有 6 个请求
    assert_eq!(parsed.requests.len(), 6);

    // 检查第一个请求有 Authorization header
    assert!(
        parsed.requests[0]
            .headers
            .iter()
            .any(|(k, _)| k.to_lowercase() == "authorization")
    );

    // 检查 @name 覆盖
    assert_eq!(
        parsed.requests[2].metadata.name,
        Some("bearer-auth-success".to_string())
    );

    assert_eq!(
        parsed.requests[4].metadata.name,
        Some("api-key-auth".to_string())
    );
}

#[test]
fn test_parse_assertions() {
    let path = PathBuf::from("examples/assertions.md");
    let parsed = MarkdownFileParser::parse_file(&path).unwrap();

    // 应该有 8 个请求
    assert_eq!(parsed.requests.len(), 8);

    // 检查第一个请求的断言
    let first_request = &parsed.requests[0];
    assert_eq!(
        first_request.metadata.name,
        Some("success-request".to_string())
    );
    assert_eq!(first_request.metadata.assertions.len(), 1);
    assert_eq!(first_request.metadata.assertions[0], "status == 200");

    // 检查有多个断言的请求
    let second_request = &parsed.requests[1];
    assert_eq!(second_request.metadata.assertions.len(), 2);

    // 检查 @name 覆盖标题的情况
    let custom_name_request = &parsed.requests[6];
    assert_eq!(
        custom_name_request.metadata.name,
        Some("this-is-custom-name".to_string())
    );
    // 而不是 "这是 Markdown 标题"

    // 检查复杂断言组合
    let full_validation = &parsed.requests[7];
    assert_eq!(
        full_validation.metadata.name,
        Some("full-validation".to_string())
    );
    assert!(full_validation.metadata.assertions.len() >= 5);
}

#[test]
fn test_parse_nested_blocks() {
    let path = PathBuf::from("examples/nested-blocks.md");
    let parsed = MarkdownFileParser::parse_file(&path).unwrap();

    // 应该有 4 个请求
    // 包括：正常的HTTP请求、包含JSON响应示例的文档、UUID生成器，还有一个4反引号的
    assert_eq!(parsed.requests.len(), 4);

    // 检查 UUID 生成器请求
    let uuid_request = parsed
        .requests
        .iter()
        .find(|r| r.metadata.name == Some("uuid-generator".to_string()))
        .expect("Should find uuid-generator request");

    assert_eq!(uuid_request.metadata.assertions.len(), 2);
}

#[test]
fn test_parse_empty_file() {
    let path = PathBuf::from("examples/empty.md");
    let parsed = MarkdownFileParser::parse_file(&path).unwrap();

    // 空文件（无 http/rest 代码块）应该返回 0 个请求
    assert_eq!(parsed.requests.len(), 0);
}

#[test]
fn test_parse_all_examples() {
    // 测试解析所有示例文件不会崩溃
    let examples = vec![
        "examples/simple-api.md",
        "examples/auth-examples.md",
        "examples/assertions.md",
        "examples/nested-blocks.md",
        "examples/empty.md",
        "examples/api-docs.md", // 之前创建的
    ];

    for example in examples {
        let path = PathBuf::from(example);
        let result = MarkdownFileParser::parse_file(&path);
        assert!(result.is_ok(), "Failed to parse {}", example);
    }
}

#[test]
fn test_request_names_from_headers() {
    // 测试从不同级别的标题提取名称
    let path = PathBuf::from("examples/simple-api.md");
    let parsed = MarkdownFileParser::parse_file(&path).unwrap();

    // 所有请求都应该有名称（从标题提取）
    for request in &parsed.requests {
        assert!(
            request.metadata.name.is_some(),
            "Request should have a name"
        );
    }
}

#[test]
fn test_mixed_http_rest_blocks() {
    // 测试混合使用 http 和 rest 代码块
    let path = PathBuf::from("examples/simple-api.md");
    let parsed = MarkdownFileParser::parse_file(&path).unwrap();

    // 两种语言标识符应该都能被识别
    assert!(!parsed.requests.is_empty());

    // 验证 http 和 rest 块都被解析
    let path2 = PathBuf::from("examples/auth-examples.md");
    let parsed2 = MarkdownFileParser::parse_file(&path2).unwrap();
    assert!(!parsed2.requests.is_empty());
}

#[test]
fn test_mock_block_preserves_json_request_body_and_clean_headers() {
    let content = r#"# API Spec
```http
POST /api/v1/orders
Authorization: Bearer my_token
Content-Type: application/json

{
  "item_count": 2,
  "sku": "SKU-9988"
}

@mock-when query.mode == test
HTTP/1.1 200 OK
Content-Type: application/json

{"status": "ok"}
```
"#;
    let parsed =
        MarkdownFileParser::parse_content(content).expect("Failed to parse markdown content");
    assert_eq!(parsed.requests.len(), 1);
    let req = &parsed.requests[0];

    // 1. 请求体必须存在，且内容正确
    let body = req
        .body
        .as_deref()
        .expect("Request body should be parsed, but was None");
    assert!(
        body.contains("\"item_count\": 2"),
        "Body should contain json content, got: {}",
        body
    );

    // 2. Headers 中绝不能把 JSON 行当成 Header 吞掉
    for (k, v) in &req.headers {
        assert!(
            !k.contains('"'),
            "Header key must not contain quotes from JSON body: ({}: {})",
            k,
            v
        );
    }

    // 3. 正常 Header 依然被保留
    assert_eq!(
        req.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("authorization"))
            .map(|(_, v)| v.as_str()),
        Some("Bearer my_token")
    );
}

#[test]
fn test_dual_role_mock_compilation_not_skipped() {
    let content = r#"# API Spec
```http
# @test
POST /api/v1/orders
Content-Type: application/json

{"item_count": 1}

@mock-when query.mode == test
HTTP/1.1 200 OK
Content-Type: application/json

{"status": "ok"}
```
"#;
    let parsed =
        MarkdownFileParser::parse_content(content).expect("Failed to parse markdown content");
    assert_eq!(parsed.requests.len(), 1);
    assert!(
        parsed.requests[0].metadata.is_test,
        "Should be recognized as a test request"
    );
    assert_eq!(parsed.requests[0].metadata.mock_variants.len(), 1);

    // 编译 Mock 路由：双角色请求绝不能因为带有 @test 就被跳过
    let routes = rupost::mock::compiler::MockCompiler::compile(&parsed);
    assert_eq!(
        routes.len(),
        1,
        "Dual-role block with @test and @mock-when should be compiled into mock routes, but got 0"
    );
    assert_eq!(routes[0].path, "/api/v1/orders");
}
