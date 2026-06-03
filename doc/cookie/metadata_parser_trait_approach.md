# 元数据解析重构方案

## 📋 现状分析

### 当前实现的问题

```rust
// 当前的 parse_metadata_line 方法（不好的做法）
fn parse_metadata_line(line: &str, ...) -> ParseResult<()> {
    if let Some(name) = line.strip_prefix("@name") {
        // 处理 @name
    } else if line.starts_with("@skip") {
        // 处理 @skip
    } else if let Some(timeout_str) = line.strip_prefix("@timeout") {
        // 处理 @timeout
    } else if let Some(assertion) = line.strip_prefix("@assert") {
        // 处理 @assert
    }
    // 未来需要添加 @capture 时，又要加一个 else if...
}
```

**问题**:
1. ❌ if-else 链难以维护
2. ❌ 添加新元数据需要修改多处
3. ❌ 没有统一的元数据类型枚举
4. ❌ 不符合开闭原则

---

## 🎯 重构目标

1. ✅ 定义统一的元数据枚举
2. ✅ 使用模式匹配代替 if-else
3. ✅ 每种元数据有独立的解析器
4. ✅ 易于添加新的元数据类型

---

## 🏗️ 新设计方案

### 方案 1: 枚举 + 模式匹配（推荐）

#### 1. 定义元数据枚举

```rust
// src/parser/metadata.rs

/// 元数据类型枚举
#[derive(Debug, Clone, PartialEq)]
pub enum Metadata {
    /// 请求名称
    /// 语法: @name <name>
    Name(String),
    
    /// 跳过标记
    /// 语法: @skip 或 @skip true/false
    Skip(bool),
    
    /// 超时时间
    /// 语法: @timeout <duration>
    Timeout(Duration),
    
    /// 断言
    /// 语法: @assert <expression>
    Assert(String),
    
    /// 变量捕获（P0 新增）
    /// 语法: @capture <var_name> from <source>
    Capture {
        var_name: String,
        source: String,
    },
    
    // === P3 预留 ===
    
    /// 依赖关系（P3）
    /// 语法: @depends_on <request_name>
    #[allow(dead_code)]
    DependsOn(String),
    
    /// 重试策略（P3）
    /// 语法: @retry <count>
    #[allow(dead_code)]
    Retry(u32),
}
```

#### 2. 元数据解析器 trait

```rust
/// 元数据解析器 trait
pub trait MetadataParser {
    /// 尝试解析元数据
    /// 返回 Some(Metadata) 如果可以解析，否则返回 None
    fn try_parse(&self, line: &str) -> Option<ParseResult<Metadata>>;
    
    /// 获取元数据标识符（如 "@name", "@skip"）
    fn directive(&self) -> &str;
}
```

#### 3. 具体的解析器实现

```rust
// === @name 解析器 ===
struct NameParser;

impl MetadataParser for NameParser {
    fn directive(&self) -> &str { "@name" }
    
    fn try_parse(&self, line: &str) -> Option<ParseResult<Metadata>> {
        line.strip_prefix("@name")
            .map(|value| Ok(Metadata::Name(value.trim().to_string())))
    }
}

// === @skip 解析器 ===
struct SkipParser;

impl MetadataParser for SkipParser {
    fn directive(&self) -> &str { "@skip" }
    
    fn try_parse(&self, line: &str) -> Option<ParseResult<Metadata>> {
        if !line.starts_with("@skip") {
            return None;
        }
        
        let value = line
            .strip_prefix("@skip")
            .and_then(|s| {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    Some(true)
                } else {
                    trimmed.parse::<bool>().ok()
                }
            })
            .unwrap_or(true);
        
        Some(Ok(Metadata::Skip(value)))
    }
}

// === @timeout 解析器 ===
struct TimeoutParser;

impl MetadataParser for TimeoutParser {
    fn directive(&self) -> &str { "@timeout" }
    
    fn try_parse(&self, line: &str) -> Option<ParseResult<Metadata>> {
        line.strip_prefix("@timeout")
            .map(|duration_str| {
                parse_duration(duration_str.trim())
                    .map(Metadata::Timeout)
            })
    }
}

// === @assert 解析器 ===
struct AssertParser;

impl MetadataParser for AssertParser {
    fn directive(&self) -> &str { "@assert" }
    
    fn try_parse(&self, line: &str) -> Option<ParseResult<Metadata>> {
        line.strip_prefix("@assert")
            .map(|expr| Ok(Metadata::Assert(expr.trim().to_string())))
    }
}

// === @capture 解析器（P0 新增）===
struct CaptureParser;

impl MetadataParser for CaptureParser {
    fn directive(&self) -> &str { "@capture" }
    
    fn try_parse(&self, line: &str) -> Option<ParseResult<Metadata>> {
        let content = line.strip_prefix("@capture")?;
        let parts: Vec<&str> = content.trim().split_whitespace().collect();
        
        // 语法: @capture <var_name> from <source>
        if parts.len() < 3 || parts[1] != "from" {
            return Some(Err(ParseError::InvalidMetadata {
                line: 0,
                message: format!("Invalid @capture syntax: {}", line),
            }));
        }
        
        Some(Ok(Metadata::Capture {
            var_name: parts[0].to_string(),
            source: parts[2].to_string(),
        }))
    }
}
```

#### 4. 元数据注册表

```rust
/// 元数据解析器注册表
pub struct MetadataRegistry {
    parsers: Vec<Box<dyn MetadataParser>>,
}

impl MetadataRegistry {
    /// 创建默认的注册表（包含所有内置解析器）
    pub fn new() -> Self {
        Self {
            parsers: vec![
                Box::new(NameParser),
                Box::new(SkipParser),
                Box::new(TimeoutParser),
                Box::new(AssertParser),
                Box::new(CaptureParser),
                // P3 时只需添加新的解析器即可！
            ],
        }
    }
    
    /// 解析元数据行
    pub fn parse(&self, line: &str) -> ParseResult<Option<Metadata>> {
        for parser in &self.parsers {
            if let Some(result) = parser.try_parse(line) {
                return result.map(Some);
            }
        }
        
        // 未识别的元数据（可以选择警告或忽略）
        Ok(None)
    }
    
    /// 应用元数据到 RequestMetadata
    pub fn apply(metadata: &Metadata, target: &mut RequestMetadata) {
        match metadata {
            Metadata::Name(name) => {
                target.name = Some(name.clone());
            }
            Metadata::Skip(skip) => {
                target.skip = *skip;
            }
            Metadata::Timeout(duration) => {
                target.timeout = Some(*duration);
            }
            Metadata::Assert(expr) => {
                target.assertions.push(expr.clone());
            }
            Metadata::Capture { var_name, source } => {
                // P0: 解析 capture 并添加到 metadata
                target.captures.push(VariableCapture::parse(var_name, source));
            }
            Metadata::DependsOn(_) => {
                // P3: 未实现
            }
            Metadata::Retry(_) => {
                // P3: 未实现
            }
        }
    }
}
```

#### 5. 重构后的 parse_metadata_line

```rust
// src/parser/http_file.rs

impl HttpFileParser {
    /// 解析元数据行（重构后）
    fn parse_metadata_line(
        line: &str,
        line_number: usize,
        metadata: &mut RequestMetadata,
        registry: &MetadataRegistry,  // 新增参数
    ) -> ParseResult<()> {
        // 统一解析
        match registry.parse(line)? {
            Some(md) => {
                MetadataRegistry::apply(&md, metadata);
                Ok(())
            }
            None => {
                // 未识别的元数据（可以选择警告）
                eprintln!("Warning [line {}]: Unrecognized metadata: {}", line_number, line);
                Ok(())
            }
        }
    }
}
```

---

## 📊 方案对比

| 特性 | 当前方案 (if-else) | 新方案 (枚举 + trait) |
|------|-------------------|---------------------|
| **可读性** | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ |
| **可维护性** | ⭐⭐ | ⭐⭐⭐⭐⭐ |
| **扩展性** | ⭐⭐ | ⭐⭐⭐⭐⭐ |
| **测试性** | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ |
| **性能** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ |

---

## 🚀 实施步骤

### Step 1: 创建新模块
- [ ] 创建 `src/parser/metadata.rs`
- [ ] 定义 `Metadata` 枚举
- [ ] 定义 `MetadataParser` trait

### Step 2: 实现解析器
- [ ] 实现现有的 4 个解析器（Name, Skip, Timeout, Assert）
- [ ] 实现新的 Capture 解析器
- [ ] 添加单元测试

### Step 3: 创建注册表
- [ ] 实现 `MetadataRegistry`
- [ ] 实现 `apply` 方法

### Step 4: 重构 HttpFileParser
- [ ] 修改 `parse_metadata_line` 使用新 API
- [ ] 更新测试

### Step 5: 扩展 RequestMetadata
- [ ] 添加 `captures` 字段
- [ ] 添加 P3 预留字段（可选）

---

## ✅ 收益

### 立即收益
1. ✅ 代码更清晰，每个元数据有独立处理器
2. ✅ 易于测试，可以单独测试每个解析器
3. ✅ 符合单一职责原则

### 长期收益
1. ✅ 添加新元数据只需：
   - 在枚举中添加新变体
   - 实现一个新的 Parser
   - 在注册表中注册
   - **无需修改 `parse_metadata_line`！**

2. ✅ P3 扩展容易：
   ```rust
   // P3 时添加新元数据
   Box::new(DependsOnParser),  // ← 只需一行
   Box::new(RetryParser),      // ← 只需一行
   ```

---

## 📝 示例

### 添加新元数据的完整流程

```rust
// 1. 在 Metadata 枚举中添加新变体
pub enum Metadata {
    // ... 现有的
    
    /// 重试策略（P3）
    Retry(u32),
}

// 2. 实现解析器
struct RetryParser;

impl MetadataParser for RetryParser {
    fn directive(&self) -> &str { "@retry" }
    
    fn try_parse(&self, line: &str) -> Option<ParseResult<Metadata>> {
        line.strip_prefix("@retry")
            .map(|count_str| {
                count_str.trim()
                    .parse::<u32>()
                    .map(Metadata::Retry)
                    .map_err(|_| ParseError::InvalidMetadata {
                        line: 0,
                        message: "Invalid retry count".to_string(),
                    })
            })
    }
}

// 3. 在注册表中注册
impl MetadataRegistry {
    pub fn new() -> Self {
        Self {
            parsers: vec![
                // ... 现有的
                Box::new(RetryParser),  // ← 添加这一行
            ],
        }
    }
}

// 4. 在 apply 方法中处理
pub fn apply(metadata: &Metadata, target: &mut RequestMetadata) {
    match metadata {
        // ... 现有的
        Metadata::Retry(count) => {
            target.retry_count = Some(*count);
        }
    }
}

// 完成！无需修改其他代码！
```

---

**推荐采用方案 1**，它平衡了简洁性和扩展性，非常适合当前项目的规模和未来的扩展需求。
