# 插件快速开始

> 🚀 **目标**: 5 分钟创建第一个 RuPost 插件  
> 📅 **更新**: 2026-02-03  
> 💡 **难度**: 入门

---

## 一、前置要求

确保你已安装：

- ✅ **Rust** >= 1.70.0
- ✅ **RuPost** >= 0.1.0
- ✅ **wasm32-wasi** 编译目标

### 安装 WASM 工具链

```bash
# 添加 WASM 编译目标
rustup target add wasm32-wasi

# （可选）安装 wasm-opt 优化工具
cargo install wasm-opt
```

---

## 二、创建第一个插件

### 步骤 1: 初始化项目

```bash
# 创建新 Rust 库项目
cargo new --lib hello-rupost
cd hello-rupost

# 或使用 RuPost CLI（Phase 2 提供）
rupost plugin new hello-rupost
```

### 步骤 2: 配置 Cargo.toml

```toml
[package]
name = "hello-rupost"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]  # 编译为动态库

[dependencies]
rupost-plugin-sdk = "1.0"  # 插件 SDK
async-trait = "0.1"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

[profile.release]
opt-level = "z"     # 优化体积
lto = true          # 链接时优化
strip = true        # 去除调试符号
```

### 步骤 3: 编写插件代码

```rust
// src/lib.rs
use rupost_plugin_sdk::*;
use async_trait::async_trait;

pub struct HelloPlugin;

#[async_trait]
impl Plugin for HelloPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "hello-rupost".to_string(),
            version: "0.1.0".to_string(),
            author: "Your Name".to_string(),
            description: "My first RuPost plugin!".to_string(),
            homepage: None,
            permissions: vec![
                Permission::ReadRequest,
                Permission::Log { max_level: LogLevel::Info },
            ],
            hooks: vec![Hook::OnRequest],
            min_rupost_version: "0.1.0".to_string(),
        }
    }
    
    async fn init(&mut self, _config: &PluginConfig) -> Result<()> {
        println!("Hello plugin initialized!");
        Ok(())
    }
    
    async fn on_request(&mut self, ctx: &mut PluginContext) -> Result<()> {
        // 在请求头中添加自定义标识
        ctx.set_header("X-Hello-Plugin", "v0.1.0");
        
        // 记录日志
        ctx.logger.info("Added custom header");
        
        Ok(())
    }
    
    async fn cleanup(&mut self) -> Result<()> {
        println!("Goodbye!");
        Ok(())
    }
}

// 导出插件（必需）
rupost_plugin_export!(HelloPlugin);
```

### 步骤 4: 创建 plugin.toml

```toml
[plugin]
name = "hello-rupost"
version = "0.1.0"
author = "Your Name"
description = "My first RuPost plugin!"

[[permissions]]
type = "ReadRequest"

[[permissions]]
type = "Log"
max_level = "Info"

[hooks]
on_request = true
```

### 步骤 5: 编译插件

```bash
# 编译为 WASM
cargo build --target wasm32-wasi --release

# 优化 WASM 体积（可选）
wasm-opt -Oz \
  target/wasm32-wasi/release/hello_rupost.wasm \
  -o hello-rupost.wasm

# 检查文件大小
ls -lh hello-rupost.wasm
```

### 步骤 6: 安装插件

```bash
# 复制到插件目录
mkdir -p ~/.rupost/plugins/hello-rupost
cp hello-rupost.wasm ~/.rupost/plugins/hello-rupost/
cp plugin.toml ~/.rupost/plugins/hello-rupost/

# 或使用 CLI 安装（Phase 2）
rupost plugin install ./hello-rupost.wasm
```

### 步骤 7: 测试插件

```bash
# 创建测试文件
cat > test.http <<EOF
GET https://httpbin.org/get
EOF

# 运行测试
rupost test test.http

# 检查输出，应该看到 X-Hello-Plugin 请求头
```

---

## 三、常见插件示例

### 3.1 自动添加认证头

```rust
pub struct AuthPlugin {
    token: String,
}

#[async_trait]
impl Plugin for AuthPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "auth-header".to_string(),
            version: "1.0.0".to_string(),
            author: "You".to_string(),
            description: "Auto add Authorization header".to_string(),
            permissions: vec![
                Permission::ModifyRequest,
                Permission::Environment {
                    allowed_keys: vec!["AUTH_TOKEN".to_string()],
                },
            ],
            hooks: vec![Hook::OnRequest],
            min_rupost_version: "0.1.0".to_string(),
            ..Default::default()
        }
    }
    
    async fn init(&mut self, config: &PluginConfig) -> Result<()> {
        // 从环境变量读取 token
        self.token = std::env::var("AUTH_TOKEN")
            .or_else(|_| config.get::<String>("token"))
            .unwrap_or_default();
        Ok(())
    }
    
    async fn on_request(&mut self, ctx: &mut PluginContext) -> Result<()> {
        if !self.token.is_empty() {
            ctx.set_header("Authorization", &format!("Bearer {}", self.token));
        }
        Ok(())
    }
    
    async fn cleanup(&mut self) -> Result<()> {
        Ok(())
    }
}
```

### 3.2 JSON 响应格式化

```rust
pub struct JsonFormatterPlugin;

#[async_trait]
impl Plugin for JsonFormatterPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "json-formatter".to_string(),
            permissions: vec![Permission::ReadResponse],
            hooks: vec![Hook::FormatOutput],
            ..Default::default()
        }
    }
    
    async fn init(&mut self, _config: &PluginConfig) -> Result<()> {
        Ok(())
    }
    
    async fn format_output(
        &self,
        response: &Response,
    ) -> Result<Option<String>> {
        // 检查是否为 JSON 响应
        if let Some(ct) = response.headers.get("content-type") {
            if ct.contains("application/json") {
                let json: serde_json::Value = response.body_json()?;
                return Ok(Some(serde_json::to_string_pretty(&json)?));
            }
        }
        Ok(None)
    }
    
    async fn cleanup(&mut self) -> Result<()> {
        Ok(())
    }
}
```

### 3.3 请求计时统计

```rust
use std::time::Instant;

pub struct TimerPlugin {
    start_time: Option<Instant>,
}

#[async_trait]
impl Plugin for TimerPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "request-timer".to_string(),
            permissions: vec![
                Permission::ReadRequest,
                Permission::ReadResponse,
                Permission::Log { max_level: LogLevel::Info },
            ],
            hooks: vec![Hook::OnRequest, Hook::OnResponse],
            ..Default::default()
        }
    }
    
    async fn on_request(&mut self, ctx: &mut PluginContext) -> Result<()> {
        self.start_time = Some(Instant::now());
        ctx.logger.info(&format!("Sending {} {}", 
            ctx.request.method, 
            ctx.request.url
        ));
        Ok(())
    }
    
    async fn on_response(&mut self, ctx: &mut PluginContext) -> Result<()> {
        if let Some(start) = self.start_time {
            let elapsed = start.elapsed();
            ctx.logger.info(&format!(
                "Response received in {:.2}ms (status: {})",
                elapsed.as_secs_f64() * 1000.0,
                ctx.response.as_ref().unwrap().status
            ));
        }
        Ok(())
    }
    
    async fn init(&mut self, _config: &PluginConfig) -> Result<()> {
        Ok(())
    }
    
    async fn cleanup(&mut self) -> Result<()> {
        Ok(())
    }
}
```

---

## 四、调试技巧

### 4.1 本地调试

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_plugin() {
        let mut plugin = HelloPlugin;
        let config = PluginConfig::default();
        
        // 测试初始化
        plugin.init(&config).await.unwrap();
        
        // 测试钩子
        let mut ctx = PluginContext::default();
        ctx.request.url = "https://example.com".parse().unwrap();
        
        plugin.on_request(&mut ctx).await.unwrap();
        
        assert_eq!(
            ctx.get_header("X-Hello-Plugin"),
            Some("v0.1.0")
        );
    }
}
```

### 4.2 日志输出

```rust
async fn on_request(&mut self, ctx: &mut PluginContext) -> Result<()> {
    // 使用不同日志级别
    ctx.logger.debug("Detailed debug info");
    ctx.logger.info("Normal operation");
    ctx.logger.warn("Something suspicious");
    ctx.logger.error("Critical error!");
    
    Ok(())
}
```

### 4.3 错误处理

```rust
async fn on_request(&mut self, ctx: &mut PluginContext) -> Result<()> {
    // 优雅处理错误
    if let Some(token) = ctx.get_variable("auth_token") {
        ctx.set_header("Authorization", &format!("Bearer {}", token));
    } else {
        ctx.logger.warn("No auth token found, skipping");
    }
    
    Ok(())
}
```

---

## 五、常见问题

### Q1: 编译后的 WASM 文件太大怎么办？

**A**: 优化步骤：

```bash
# 1. 使用 release 配置
cargo build --target wasm32-wasi --release

# 2. 启用 LTO 和 strip（在 Cargo.toml）
[profile.release]
opt-level = "z"
lto = true
strip = true

# 3. 使用 wasm-opt
wasm-opt -Oz input.wasm -o output.wasm

# 4. 移除未使用的依赖
cargo tree
```

### Q2: 如何访问环境变量？

**A**: 声明 `Environment` 权限并使用标准库：

```rust
// plugin.toml
[[permissions]]
type = "Environment"
allowed_keys = ["API_KEY"]

// lib.rs
async fn init(&mut self, _config: &PluginConfig) -> Result<()> {
    self.api_key = std::env::var("API_KEY").ok();
    Ok(())
}
```

### Q3: 插件可以发起 HTTP 请求吗？

**A**: 可以，需要 `Network` 权限：

```rust
// plugin.toml
[[permissions]]
type = "Network"
allowed_domains = ["api.example.com"]
allow_https = true
allow_http = false

// lib.rs
async fn on_request(&mut self, ctx: &mut PluginContext) -> Result<()> {
    let response = ctx.http_client().get("https://api.example.com").await?;
    // 处理响应
    Ok(())
}
```

### Q4: 如何在插件间共享数据？

**A**: 使用 `PluginContext` 的变量存储：

```rust
// 插件 A 设置变量
ctx.set_variable("shared_token", token);

// 插件 B 读取变量
if let Some(token) = ctx.get_variable("shared_token") {
    // 使用 token
}
```

---

## 六、下一步

✅ **完成快速开始后**：

1. 📖 阅读 [API 参考文档](api_reference.md)
2. 🔐 了解 [权限系统](permissions.md)
3. 🏗️ 学习 [架构设计](architecture.md)
4. 💡 查看 [示例插件](examples/)

---

**需要帮助？**
- 📝 [提交 Issue](https://github.com/rupost/rupost/issues)
- 💬 [加入讨论](https://discord.gg/rupost)
- 📧 [联系我们](mailto:support@rupost.dev)

**祝你开发愉快！** 🚀
