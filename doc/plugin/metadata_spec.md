# 插件元数据规范

> 📋 **规范版本**: v1.0  
> 📅 **最后更新**: 2026-02-03  
> 🎯 **格式**: TOML

---

## 一、概述

每个 RuPost 插件必须包含一个 `plugin.toml` 文件，用于描述插件的元数据、依赖和权限需求。

### 1.1 文件位置

```
my-plugin/
├── plugin.toml          # 元数据文件（必需）
├── plugin.wasm          # 编译后的 WASM 模块
└── README.md            # 插件文档（推荐）
```

### 1.2 最小示例

```toml
[plugin]
name = "my-plugin"
version = "1.0.0"
author = "Your Name"
description = "A simple RuPost plugin"

[[permissions]]
type = "ReadRequest"
```

---

## 二、完整规范

### 2.1 [plugin] 节

**必需字段**：

```toml
[plugin]
# 插件唯一标识符（小写字母、数字、连字符）
name = "my-awesome-plugin"

# 语义化版本号（遵循 SemVer 2.0）
version = "1.2.3"

# 作者信息（姓名或组织）
author = "John Doe"

# 简短描述（< 100 字符）
description = "Automatically inject authentication headers"
```

**可选字段**：

```toml
# 主页 URL
homepage = "https://github.com/your-name/my-plugin"

# 仓库 URL
repository = "https://github.com/your-name/my-plugin"

# 许可证（SPDX 标识符）
license = "MIT"

# 关键词（用于搜索）
keywords = ["auth", "security", "oauth"]

# 分类
categories = ["authentication", "utilities"]

# 最低 RuPost 版本要求
min_rupost_version = "0.1.0"

# 最高兼容 RuPost 版本
max_rupost_version = "0.9.99"

# API 版本（插件 SDK 版本）
api_version = "1.0.0"
```

### 2.2 [[permissions]] 节

**数组格式**，每个权限一个条目：

```toml
# 简单权限
[[permissions]]
type = "ReadRequest"

[[permissions]]
type = "ModifyRequest"

# 带参数的权限
[[permissions]]
type = "Network"
allowed_domains = ["api.example.com", "*.googleapis.com"]
allow_https = true
allow_http = false

[[permissions]]
type = "FileSystem"
allowed_paths = ["/tmp/rupost-cache"]
read = true
write = false

[[permissions]]
type = "Environment"
allowed_keys = ["API_KEY", "AUTH_TOKEN"]

[[permissions]]
type = "Log"
max_level = "Info"
```

**权限类型完整列表**：

| 类型 | 必需参数 | 可选参数 |
|------|----------|----------|
| `ReadRequest` | - | - |
| `ModifyRequest` | - | - |
| `ReadResponse` | - | - |
| `ModifyResponse` | - | - |
| `Network` | `allowed_domains`, `allow_https`, `allow_http` | - |
| `FileSystem` | `allowed_paths`, `read`, `write` | - |
| `Environment` | `allowed_keys` | - |
| `Log` | `max_level` | - |
| `Execute` | `allowed_commands` | - |

### 2.3 [config] 节

插件默认配置（用户可覆盖）：

```toml
[config]
enabled = true
priority = 10

[config.settings]
custom_header = "X-Custom-Value"
timeout = 5000
retry_count = 3
```

### 2.4 [[dependencies]] 节

插件依赖（Phase 2+ 功能）：

```toml
[[dependencies]]
name = "rupost-http-utils"
version = "^1.0"

[[dependencies]]
name = "json-parser"
version = "~2.3.0"
```

**版本约束语法**（遵循 Cargo 语法）：

- `1.2.3` - 精确版本
- `^1.2` - 兼容版本（>= 1.2.0, < 2.0.0）
- `~1.2.3` - 补丁版本（>= 1.2.3, < 1.3.0）
- `>= 1.2, < 1.5` - 范围约束

### 2.5 [hooks] 节

声明插件实现的钩子：

```toml
[hooks]
on_request = true
on_response = true
on_assert = false
format_output = true
```

### 2.6 [metadata] 节

额外元数据（自由字段）：

```toml
[metadata]
icon = "🔐"
color = "#3498db"
tags = ["oauth", "jwt", "authentication"]

[metadata.support]
email = "support@example.com"
chat = "https://discord.gg/rupost"
issues = "https://github.com/your-name/my-plugin/issues"
```

---

## 三、完整示例

### 3.1 认证插件示例

```toml
# plugin.toml
[plugin]
name = "oauth-authenticator"
version = "2.1.0"
author = "Security Team <security@example.com>"
description = "Automatically handle OAuth 2.0 authentication flow"
homepage = "https://github.com/rupost/oauth-plugin"
repository = "https://github.com/rupost/oauth-plugin"
license = "Apache-2.0"
keywords = ["oauth", "authentication", "security"]
categories = ["authentication"]
min_rupost_version = "0.1.0"
api_version = "1.0.0"

[[permissions]]
type = "ReadRequest"

[[permissions]]
type = "ModifyRequest"

[[permissions]]
type = "Network"
allowed_domains = ["auth.example.com", "oauth.googleapis.com"]
allow_https = true
allow_http = false

[[permissions]]
type = "Environment"
allowed_keys = ["OAUTH_CLIENT_ID", "OAUTH_CLIENT_SECRET"]

[[permissions]]
type = "FileSystem"
allowed_paths = ["~/.rupost/oauth-tokens"]
read = true
write = true

[[permissions]]
type = "Log"
max_level = "Info"

[config]
enabled = true
priority = 100

[config.settings]
token_refresh_threshold = 300  # 5 minutes
auto_refresh = true
token_cache_path = "~/.rupost/oauth-tokens"

[hooks]
on_request = true
on_response = true

[metadata]
icon = "🔐"
color = "#e74c3c"
tags = ["oauth2", "jwt", "bearer-token"]

[metadata.support]
email = "oauth-support@example.com"
docs = "https://docs.example.com/oauth-plugin"
issues = "https://github.com/rupost/oauth-plugin/issues"
```

### 3.2 格式化插件示例

```toml
[plugin]
name = "json-pretty-formatter"
version = "1.0.0"
author = "Community"
description = "Pretty-print JSON responses with syntax highlighting"
homepage = "https://github.com/community/json-formatter"
license = "MIT"
keywords = ["json", "formatter", "pretty-print"]
categories = ["utilities", "formatting"]
min_rupost_version = "0.1.0"
api_version = "1.0.0"

[[permissions]]
type = "ReadResponse"

[[permissions]]
type = "Log"
max_level = "Debug"

[config]
enabled = true
priority = 5

[config.settings]
indent_size = 2
color_scheme = "monokai"
max_depth = 10

[hooks]
format_output = true

[metadata]
icon = "🎨"
color = "#2ecc71"
```

---

## 四、验证规则

### 4.1 必需字段验证

```rust
pub fn validate_metadata(toml: &str) -> Result<PluginMetadata> {
    let parsed: TomlValue = toml::from_str(toml)?;
    
    // 检查必需字段
    let plugin = parsed.get("plugin")
        .ok_or("Missing [plugin] section")?;
    
    let name = plugin.get("name")
        .and_then(|v| v.as_str())
        .ok_or("Missing plugin.name")?;
    
    let version = plugin.get("version")
        .and_then(|v| v.as_str())
        .ok_or("Missing plugin.version")?;
    
    // 验证名称格式
    validate_plugin_name(name)?;
    
    // 验证版本格式
    semver::Version::parse(version)?;
    
    Ok(PluginMetadata { /* ... */ })
}
```

### 4.2 名称规则

```rust
fn validate_plugin_name(name: &str) -> Result<()> {
    let regex = Regex::new(r"^[a-z0-9-]+$")?;
    
    if !regex.is_match(name) {
        return Err(anyhow!(
            "Plugin name must contain only lowercase letters, numbers, and hyphens"
        ));
    }
    
    if name.len() < 3 || name.len() > 50 {
        return Err(anyhow!(
            "Plugin name must be between 3 and 50 characters"
        ));
    }
    
    if name.starts_with('-') || name.ends_with('-') {
        return Err(anyhow!(
            "Plugin name cannot start or end with a hyphen"
        ));
    }
    
    Ok(())
}
```

### 4.3 版本兼容性检查

```rust
fn check_compatibility(
    plugin_version: &str,
    rupost_version: &str,
) -> Result<()> {
    let plugin = semver::Version::parse(plugin_version)?;
    let rupost = semver::Version::parse(rupost_version)?;
    
    // 检查主版本号兼容性
    if plugin.major > rupost.major {
        return Err(anyhow!(
            "Plugin requires RuPost v{}, but current version is v{}",
            plugin.major,
            rupost.major
        ));
    }
    
    Ok(())
}
```

---

## 五、CLI 工具支持

### 5.1 验证元数据

```bash
$ rupost plugin validate plugin.toml

✅ 元数据验证通过

插件信息：
  名称：oauth-authenticator
  版本：2.1.0
  作者：Security Team

权限需求（5 项）：
  🟢 ReadRequest
  🟡 ModifyRequest
  🟠 Network (2 域名)
  🟠 Environment (2 变量)
  🔴 FileSystem (1 路径)
```

### 5.2 生成模板

```bash
$ rupost plugin init my-new-plugin

创建插件项目：my-new-plugin
  ✅ plugin.toml
  ✅ src/lib.rs
  ✅ Cargo.toml
  ✅ README.md

下一步：
  cd my-new-plugin
  cargo build --target wasm32-wasi
```

---

## 六、迁移指南

### 6.1 从 v0.x 迁移到 v1.0

**变更**：
- `rupost_version` → `min_rupost_version`
- `permission` → `permissions`（数组格式）

**旧格式**（v0.x）：
```toml
[plugin]
rupost_version = "0.1.0"

[permission]
read_request = true
modify_request = true
```

**新格式**（v1.0）：
```toml
[plugin]
min_rupost_version = "0.1.0"

[[permissions]]
type = "ReadRequest"

[[permissions]]
type = "ModifyRequest"
```

---

## 七、最佳实践

### 7.1 版本号管理

✅ **DO**:
- 遵循 SemVer 2.0 规范
- 破坏性变更时增加主版本号
- 新功能时增加次版本号
- 修复 bug 时增加补丁版本号

### 7.2 描述编写

✅ **DO**:
- 使用简洁明了的语言
- 突出核心功能
- 避免营销术语

❌ **DON'T**:
- 描述过长（> 100 字符）
- 使用夸张词汇（"best"、"ultimate"）

### 7.3 权限最小化

✅ **DO**:
- 仅请求必要权限
- 使用精确的域名/路径限制
- 在文档中说明每个权限的用途

---

**规范状态**: ✅ 稳定（Phase 1）  
**下次审查**: Phase 2 开始前  
**文档维护者**: RuPost 标准化委员会
