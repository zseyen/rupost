// 端到端示例：解析 .http 文件并执行请求
use rupost::http::{Client, Request};
use rupost::parser;

#[tokio::main]
async fn main() -> rupost::Result<()> {
    println!("🚀 RuPost - 端到端测试：解析并执行 HTTP 请求\n");
    println!("{}\n", "=".repeat(60));

    // 1. 解析文件
    println!("📄 步骤 1: 解析 examples/basic.http");
    let parsed = parser::parse_file("examples/basic.http")?;
    println!("   ✅ 解析成功，找到 {} 个请求\n", parsed.requests.len());

    // 2. 转换为可执行请求
    println!("🔄 步骤 2: 转换为可执行请求");
    let requests: Vec<Request> = parsed
        .requests
        .into_iter()
        .map(|r| r.try_into())
        .collect::<rupost::Result<_>>()?;
    println!("   ✅ 成功转换 {} 个请求\n", requests.len());

    // 3. 执行第一个请求
    println!("📤 步骤 3: 执行第一个请求");
    let client = Client::new();

    if let Some(request) = requests.into_iter().next() {
        println!("   方法: {}", request.method.as_str());
        println!("   URL:  {}", request.url);
        println!("   Headers: {}", request.headers.len());

        println!("\n   发送请求...");
        let response = client.execute(request).await?;

        println!("\n✨ 步骤 4: 响应结果");
        println!("   状态码: {}", response.status.code());
        println!("   耗时:   {}ms", response.duration.as_millis());
        println!("   Headers: {}", response.headers.len());
        println!("   Body:    {} 字节", response.body.len());

        if response.is_success() {
            println!("\n🎉 成功！MVP 最小可用产品已实现");
            println!("   ✅ 文件解析");
            println!("   ✅ 请求转换");
            println!("   ✅ HTTP 执行");
        }
    }

    println!("\n{}", "=".repeat(60));
    Ok(())
}
