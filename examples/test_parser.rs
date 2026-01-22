// 测试解析器的简单程序
use rupost::parser;

fn main() {
    println!("🧪 测试 RuPost 解析器\n");

    // 测试 1: 解析 basic.http
    println!("📄 测试 1: 解析 examples/basic.http");
    match parser::parse_file("examples/basic.http") {
        Ok(parsed) => {
            println!("✅ 解析成功！");
            println!("   - 找到 {} 个请求", parsed.requests.len());
            for (i, req) in parsed.requests.iter().enumerate() {
                println!(
                    "   - 请求 {}: {} {}",
                    i + 1,
                    req.method_or_default(),
                    req.url
                );
                println!("     Headers: {}", req.headers.len());
                if req.body.is_some() {
                    println!("     Body: 有");
                }
            }
        }
        Err(e) => {
            println!("❌ 解析失败: {}", e);
        }
    }

    println!();
    println!("{}", "=".repeat(50));
    println!();

    // 测试 2: 解析字符串内容
    println!("📝 测试 2: 解析字符串内容");
    let content = r#"
GET http://example.com/api/users
Accept: application/json

###

POST http://example.com/api/users
Content-Type: application/json

{"name": "Alice", "role": "admin"}
"#;

    match parser::parse_content(content) {
        Ok(parsed) => {
            println!("✅ 解析成功！");
            println!("   - 找到 {} 个请求", parsed.requests.len());
            for (i, req) in parsed.requests.iter().enumerate() {
                println!(
                    "   - 请求 {}: {} {}",
                    i + 1,
                    req.method_or_default(),
                    req.url
                );
            }
        }
        Err(e) => {
            println!("❌ 解析失败: {}", e);
        }
    }

    println!("\n✨ 测试完成！");
}
