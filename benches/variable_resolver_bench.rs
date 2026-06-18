use criterion::{Criterion, black_box, criterion_group, criterion_main};
use rupost::variable::{VariableContext, VariableResolver};

fn bench_variable_resolver(c: &mut Criterion) {
    let mut ctx = VariableContext::new();
    ctx.insert("base_url", "https://api.example.com");
    ctx.insert("version", "v1");
    ctx.insert("userId", "12345");
    ctx.insert("token", "my-oauth-bearer-token-string");

    let simple_template = "GET {{base_url}}/users/{{userId}}";
    let complex_template = "GET {{base_url}}/{{version}}/users/{{userId}}?token={{token}}&uuid={{$uuid}}&time={{$timestamp}}";

    c.bench_function("resolver_resolve_simple", |b| {
        b.iter(|| VariableResolver::resolve(black_box(simple_template), black_box(&ctx)))
    });

    c.bench_function("resolver_resolve_complex_dynamic", |b| {
        b.iter(|| VariableResolver::resolve(black_box(complex_template), black_box(&ctx)))
    });
}

criterion_group!(benches, bench_variable_resolver);
criterion_main!(benches);
