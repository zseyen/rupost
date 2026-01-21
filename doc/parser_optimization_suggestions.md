# Parser 模块优化建议

## 优化项 1: 将 HttpFileParser 改为模块级函数

### 当前设计
```rust
// src/parser/http_file.rs
pub struct HttpFileParser;  // 空结构体，无字段

impl HttpFileParser {
    pub fn parse_file<P: AsRef<Path>>(path: P) -> ParseResult<ParsedFile> { ... }
    pub fn parse_content(content: &str) -> ParseResult<ParsedFile> { ... }
    fn split_by_separator(content: &str) -> Vec<(String, usize)> { ... }
    // ...
}
```

### 问题
- ❌ 空 struct 无实际状态，不需要实例化
- ❌ 所有方法都是关联函数，未使用 `self`
- ❌ 增加了不必要的抽象层
- ❌ 不符合 YAGNI 原则

### 建议改进
```rust
// src/parser/http_file.rs

/// 从文件路径解析 HTTP 请求
pub fn parse_file<P: AsRef<Path>>(path: P) -> ParseResult<ParsedFile> {
    let content = std::fs::read_to_string(path.as_ref())?;
    let mut parsed = parse_content(&content)?;
    parsed.source_path = Some(path.as_ref().to_path_buf());
    Ok(parsed)
}

/// 从字符串内容解析 HTTP 请求
pub fn parse_content(content: &str) -> ParseResult<ParsedFile> {
    // ...
}

// 私有辅助函数
fn split_by_separator(content: &str) -> Vec<(String, usize)> {
    // ...
}

fn parse_request_block(block: &str, start_line: usize) -> ParseResult<Option<ParsedRequest>> {
    // ...
}
```

### 优势
- ✅ 更符合 Rust 惯用法（如 `std::fs::read_to_string`）
- ✅ 减少不必要的代码
- ✅ API 更直观：`http_file::parse_file()` vs `HttpFileParser::parse_file()`
- ✅ 遵循简单设计原则

### 使用方式对比

**之前**：
```rust
use rupost::parser::HttpFileParser;
let parsed = HttpFileParser::parse_file("test.http")?;
```

**之后**：
```rust
use rupost::parser::http_file;
let parsed = http_file::parse_file("test.http")?;

// 或者直接导入函数
use rupost::parser::http_file::parse_file;
let parsed = parse_file("test.http")?;
```

### 实施优先级
**优先级**: P2 (优化，非紧急)

**原因**:
- 当前设计已经可以工作
- 不影响核心功能
- 可在后续重构时处理

### 未来扩展场景
如果将来需要添加配置选项（如最大文件大小、编码等），可以再引入 struct：

```rust
pub struct HttpFileParser {
    max_file_size: usize,
    encoding: Encoding,
}

impl HttpFileParser {
    pub fn new() -> Self {
        Self {
            max_file_size: 10 * 1024 * 1024, // 10MB
            encoding: Encoding::Utf8,
        }
    }
    
    pub fn with_max_size(mut self, size: usize) -> Self {
        self.max_file_size = size;
        self
    }
    
    pub fn parse_file(&self, path: &Path) -> ParseResult<ParsedFile> {
        // 使用 self.max_file_size 等配置
        // ...
    }
}
```

---

## 优化项 2: 元数据解析实现（第二迭代）

### 当前状态
- ✅ 数据结构已定义（`RequestMetadata`）
- ⚠️ 解析器只识别但未解析 `@` 开头的行

### 待实现
```rust
fn parse_metadata(line: &str) -> Option<MetadataType> {
    if let Some(name) = line.strip_prefix("@name") {
        Some(MetadataType::Name(name.trim().to_string()))
    } else if line.starts_with("@skip") {
        Some(MetadataType::Skip)
    } else if let Some(timeout) = line.strip_prefix("@timeout") {
        // 解析时间，如 "5s", "1000ms"
        Some(MetadataType::Timeout(parse_duration(timeout)?))
    } else {
        None
    }
}
```

---

## 优化项 3: Markdown 解析器（第二迭代）

### 目标
提取 Markdown 文件中的 HTTP 代码块

### 实现策略
```rust
// src/parser/markdown_file.rs

pub fn parse_file<P: AsRef<Path>>(path: P) -> ParseResult<ParsedFile> {
    let content = std::fs::read_to_string(path.as_ref())?;
    let mut parsed = parse_content(&content)?;
    parsed.source_path = Some(path.as_ref().to_path_buf());
    Ok(parsed)
}

pub fn parse_content(content: &str) -> ParseResult<ParsedFile> {
    let mut file = ParsedFile::new();
    let mut in_code_block = false;
    let mut current_block = String::new();
    let mut last_heading = None;
    let mut block_start_line = 0;
    
    for (line_num, line) in content.lines().enumerate() {
        if line.starts_with("```http") || line.starts_with("```rest") {
            in_code_block = true;
            current_block.clear();
            block_start_line = line_num + 2;
        } else if in_code_block && line.starts_with("```") {
            in_code_block = false;
            // 复用 http_file 解析器
            if let Ok(mut parsed) = http_file::parse_content(&current_block) {
                // 如果有标题，作为请求名称
                if let (Some(heading), Some(req)) = (last_heading.as_ref(), parsed.requests.first_mut()) {
                    req.metadata.name = Some(heading.clone());
                }
                file.requests.extend(parsed.requests);
            }
        } else if in_code_block {
            current_block.push_str(line);
            current_block.push('\n');
        } else if line.starts_with('#') {
            // 提取标题
            last_heading = Some(line.trim_start_matches('#').trim().to_string());
        }
    }
    
    Ok(file)
}
```

---

## 总结

| 优化项 | 优先级 | 影响范围 | 建议时机 |
|--------|--------|----------|----------|
| HttpFileParser 改为模块函数 | P2 | API 设计 | 后续重构 |
| 元数据解析 | P1 | 功能完整性 | 第二迭代 |
| Markdown 解析器 | P1 | 差异化功能 | 第二迭代 |

**当前建议**: 优先实现 Stage 3（请求构建器），完成 MVP 最小可用产品。
