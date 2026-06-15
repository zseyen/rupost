use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rupost::parser::{HttpFileParser, MarkdownFileParser};

fn bench_parsers(c: &mut Criterion) {
    let simple_http = r#"
GET https://httpbin.org/get
X-Test: value
"#;

    let complex_http = r#"
### Login Request
@name = login
@timeout = 3000ms
POST https://httpbin.org/post
Content-Type: application/json

{
    "username": "admin",
    "password": "password123"
}

@assert status == 200
@capture auth_token from body.token
"#;

    let md_content = r#"
# Test Suite

```http
### Get User Profile
@name = get_profile
GET https://httpbin.org/headers
Authorization: Bearer {{auth_token}}

@assert status == 200
```
"#;

    c.bench_function("http_parser_simple", |b| {
        b.iter(|| HttpFileParser::parse_content(black_box(simple_http)).unwrap())
    });

    c.bench_function("http_parser_complex", |b| {
        b.iter(|| HttpFileParser::parse_content(black_box(complex_http)).unwrap())
    });

    c.bench_function("markdown_parser", |b| {
        b.iter(|| MarkdownFileParser::parse_content(black_box(md_content)).unwrap())
    });
}

criterion_group!(benches, bench_parsers);
criterion_main!(benches);
