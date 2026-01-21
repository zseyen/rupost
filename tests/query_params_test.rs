use rupost::http::Request;

#[test]
fn test_query_params_storage() {
    let request = Request::new("GET", "http://example.com")
        .unwrap()
        .with_query("q", "search")
        .with_query("page", "1")
        .with_query("limit", "10");

    assert_eq!(request.query_params.len(), 3);
    assert_eq!(request.query_params.get("q"), Some(&"search".to_string()));
    assert_eq!(request.query_params.get("page"), Some(&"1".to_string()));
    assert_eq!(request.query_params.get("limit"), Some(&"10".to_string()));
}

#[test]
fn test_query_params_override() {
    let request = Request::new("GET", "http://example.com")
        .unwrap()
        .with_query("q", "first")
        .with_query("q", "second"); // 应该覆盖第一个值

    assert_eq!(request.query_params.len(), 1);
    assert_eq!(request.query_params.get("q"), Some(&"second".to_string()));
}

#[test]
fn test_empty_query_params() {
    let request = Request::new("GET", "http://example.com").unwrap();

    assert_eq!(request.query_params.len(), 0);
    assert!(request.query_params.is_empty());
}
