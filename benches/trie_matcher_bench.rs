use criterion::{Criterion, black_box, criterion_group, criterion_main};
use rupost::mock::matcher::{MockMatcher, MockRequest, TrieRouteMatcher};
use rupost::mock::variant::{CompareOp, ConditionSource, MockVariant, VariantCondition};
use std::collections::HashMap;

fn bench_trie_matcher(c: &mut Criterion) {
    let mut matcher = TrieRouteMatcher::new();

    // 1. 精确匹配路由
    matcher.add_route(
        "GET",
        "/api/v1/health",
        vec![MockVariant {
            condition: None,
            status: 200,
            headers: HashMap::new(),
            response_body: "OK".to_string(),
        }],
    );

    // 2. 带路径参数和 Header 条件变体匹配路由
    let admin_cond = VariantCondition {
        source: ConditionSource::Header,
        key: "X-Role".to_string(),
        operator: CompareOp::Equals,
        expected_value: "Admin".to_string(),
    };

    matcher.add_route(
        "POST",
        "/api/v1/users/:id",
        vec![
            MockVariant {
                condition: Some(admin_cond),
                status: 200,
                headers: HashMap::new(),
                response_body: "{\"role\":\"admin\"}".to_string(),
            },
            MockVariant {
                condition: None,
                status: 403,
                headers: HashMap::new(),
                response_body: "{\"error\":\"forbidden\"}".to_string(),
            },
        ],
    );

    // 3. 带 Body JSONPath 变体匹配路由
    let json_cond = VariantCondition {
        source: ConditionSource::Body,
        key: "$.user.profile.age".to_string(),
        operator: CompareOp::Equals,
        expected_value: "18".to_string(),
    };
    matcher.add_route(
        "POST",
        "/api/v1/profile",
        vec![MockVariant {
            condition: Some(json_cond),
            status: 200,
            headers: HashMap::new(),
            response_body: "{\"eligible\":true}".to_string(),
        }],
    );

    // 准备不同场景的 requests
    let req_simple = MockRequest {
        method: "GET".to_string(),
        path: "/api/v1/health".to_string(),
        headers: HashMap::new(),
        query: HashMap::new(),
        body: String::new(),
    };

    let mut admin_headers = HashMap::new();
    admin_headers.insert("X-Role".to_string(), "Admin".to_string());
    let req_params_admin = MockRequest {
        method: "POST".to_string(),
        path: "/api/v1/users/99".to_string(),
        headers: admin_headers,
        query: HashMap::new(),
        body: String::new(),
    };

    let req_json_body = MockRequest {
        method: "POST".to_string(),
        path: "/api/v1/profile".to_string(),
        headers: HashMap::new(),
        query: HashMap::new(),
        body: r#"{"user":{"profile":{"age":18}}}"#.to_string(),
    };

    c.bench_function("trie_match_simple", |b| {
        b.iter(|| matcher.match_request(black_box(&req_simple)).unwrap())
    });

    c.bench_function("trie_match_params_and_header", |b| {
        b.iter(|| matcher.match_request(black_box(&req_params_admin)).unwrap())
    });

    c.bench_function("trie_match_jsonpath_body", |b| {
        b.iter(|| matcher.match_request(black_box(&req_json_body)).unwrap())
    });
}

criterion_group!(benches, bench_trie_matcher);
criterion_main!(benches);
