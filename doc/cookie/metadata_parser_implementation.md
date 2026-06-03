# 元数据解析器实现方案

## ✅ 当前采用方案：静态 Match

基于项目当前状态（5 个元数据，1-2 人团队）和性能考虑，采用**静态 match 方案**。

### 核心实现

```rust
// src/parser/metadata.rs

use crate::parser::types::{ParseError, ParseResult, Metadata, RequestMetadata};
use crate::variable::capture::VariableCapture;
use std::time::Duration;

/// 元数据类型枚举
#[derive(Debug, Clone, PartialEq)]
pub enum Metadata {
    /// 请求名称
    Name(String),
    
    /// 跳过标记
    Skip(bool),
    
    /// 超时时间
    Timeout(Duration),
    
    /// 断言
    Assert(String),
    
    /// 变量捕获
    Capture {
        var_name: String,
        source: String,
    },
}

/// 主解析函数（统一入口）
pub fn parse_metadata(line: &str) -> ParseResult<Option<Metadata>> {
    let line = line.trim();
    
    if line.starts_with("@name") {
        parse_name(line).map(Some)
    } else if line.starts_with("@skip") {
        parse_skip(line).map(Some)
    } else if line.starts_with("@timeout") {
        parse_timeout(line).map(Some)
    } else if line.starts_with("@assert") {
        parse_assert(line).map(Some)
    } else if line.starts_with("@capture") {
        parse_capture(line).map(Some)
    } else {
        Ok(None) // 未识别的元数据
    }
}

/// 应用元数据到 RequestMetadata
#[inline]
pub fn apply_metadata(metadata: &Metadata, target: &mut RequestMetadata) {
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
            target.captures.push(VariableCapture::parse(var_name, source));
        }
    }
}

// === 各个解析器实现 ===

fn parse_name(line: &str) -> ParseResult<Metadata> {
    Ok(Metadata::Name(
        line.strip_prefix("@name")
            .unwrap()
            .trim()
            .to_string()
    ))
}

fn parse_skip(line: &str) -> ParseResult<Metadata> {
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
    
    Ok(Metadata::Skip(value))
}

fn parse_timeout(line: &str) -> ParseResult<Metadata> {
    let duration_str = line.strip_prefix("@timeout")
        .unwrap()
        .trim();
    
    let duration = parse_duration(duration_str)?;
    Ok(Metadata::Timeout(duration))
}

fn parse_assert(line: &str) -> ParseResult<Metadata> {
    Ok(Metadata::Assert(
        line.strip_prefix("@assert")
            .unwrap()
            .trim()
            .to_string()
    ))
}

fn parse_capture(line: &str) -> ParseResult<Metadata> {
    let content = line.strip_prefix("@capture")
        .unwrap()
        .trim();
    
    let parts: Vec<&str> = content.split_whitespace().collect();
    
    // 语法: @capture <var_name> from <source>
    if parts.len() < 3 || parts[1] != "from" {
        return Err(ParseError::InvalidMetadata {
            line: 0,
            message: format!(
                "Invalid @capture syntax. Expected: @capture <var> from <source>"
            ),
        });
    }
    
    Ok(Metadata::Capture {
        var_name: parts[0].to_string(),
        source: parts[2].to_string(),
    })
}

/// 解析时间字符串（支持 "5s", "1000ms", "2m"）
fn parse_duration(s: &str) -> ParseResult<Duration> {
    let s = s.trim();
    
    if let Some(ms) = s.strip_suffix("ms") {
        let millis: u64 = ms.parse().map_err(|_| ParseError::InvalidMetadata {
            line: 0,
            message: format!("Invalid duration: {}", s),
        })?;
        Ok(Duration::from_millis(millis))
    } else if let Some(sec) = s.strip_suffix('s') {
        let secs: u64 = sec.parse().map_err(|_| ParseError::InvalidMetadata {
            line: 0,
            message: format!("Invalid duration: {}", s),
        })?;
        Ok(Duration::from_secs(secs))
    } else if let Some(min) = s.strip_suffix('m') {
        let mins: u64 = min.parse().map_err(|_| ParseError::InvalidMetadata {
            line: 0,
            message: format!("Invalid duration: {}", s),
        })?;
        Ok(Duration::from_secs(mins * 60))
    } else {
        Err(ParseError::InvalidMetadata {
            line: 0,
            message: format!("Duration must end with 'ms', 's', or 'm': {}", s),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_name() {
        let result = parse_metadata("@name Test Request").unwrap().unwrap();
        assert!(matches!(result, Metadata::Name(ref s) if s == "Test Request"));
    }
    
    #[test]
    fn test_parse_skip() {
        let result = parse_metadata("@skip").unwrap().unwrap();
        assert!(matches!(result, Metadata::Skip(true)));
        
        let result = parse_metadata("@skip false").unwrap().unwrap();
        assert!(matches!(result, Metadata::Skip(false)));
    }
    
    #[test]
    fn test_parse_timeout() {
        let result = parse_metadata("@timeout 5s").unwrap().unwrap();
        assert!(matches!(result, Metadata::Timeout(d) if d == Duration::from_secs(5)));
    }
    
    #[test]
    fn test_parse_assert() {
        let result = parse_metadata("@assert status == 200").unwrap().unwrap();
        assert!(matches!(result, Metadata::Assert(ref s) if s == "status == 200"));
    }
    
    #[test]
    fn test_parse_capture() {
        let result = parse_metadata("@capture token from body.token").unwrap().unwrap();
        assert!(matches!(
            result,
            Metadata::Capture { ref var_name, ref source }
            if var_name == "token" && source == "body.token"
        ));
    }
    
    #[test]
    fn test_parse_capture_invalid() {
        let result = parse_metadata("@capture invalid syntax");
        assert!(result.is_err());
    }
    
    #[test]
    fn test_parse_unrecognized() {
        let result = parse_metadata("@unknown directive").unwrap();
        assert!(result.is_none());
    }
}
```

### 在 HttpFileParser 中使用

```rust
// src/parser/http_file.rs

use crate::parser::metadata::{parse_metadata, apply_metadata};

impl HttpFileParser {
    fn parse_metadata_line(
        line: &str,
        line_number: usize,
        metadata: &mut RequestMetadata,
    ) -> ParseResult<()> {
        match parse_metadata(line)? {
            Some(md) => {
                apply_metadata(&md, metadata);
                Ok(())
            }
            None => {
                // 未识别的元数据（可选：警告）
                eprintln!("Warning [line {}]: Unrecognized metadata: {}", line_number, line);
                Ok(())
            }
        }
    }
}
```

---

## 📚 未来演进路径

### 何时重构为 Trait 方案

触发条件（满足任一即可）:
1. **元数据数量 > 10 个**
2. **团队规模 > 3 人**
3. **频繁出现 Git 冲突**
4. **if-else 链维护成本高**

### 重构步骤

参考文档：
- `doc/metadata_parser_trait_approach.md` - Trait 方案详细实现
- `doc/metadata_parser_comparison.md` - 两种方案对比分析

预计工作量：1-2 天

---

## ✅ 当前方案优势

1. **性能最优**: 5-15 ns（比 trait 快 3-5 倍）
2. **代码简单**: 直观易懂，学习曲线低
3. **适合 MVP**: 快速实现，快速迭代
4. **编译快**: 无 vtable 开销
5. **可测试**: 每个解析函数独立可测

---

## 📝 添加新元数据的步骤

1. 在 `Metadata` 枚举中添加新变体
2. 在 `parse_metadata` 中添加新分支
3. 实现对应的 `parse_xxx` 函数
4. 在 `apply_metadata` 中添加处理
5. 添加测试

**示例**：添加 `@retry` 元数据:

```rust
// 1. 枚举
pub enum Metadata {
    // ...
    Retry(u32),
}

// 2. 主函数
pub fn parse_metadata(line: &str) -> ParseResult<Option<Metadata>> {
    // ...
    } else if line.starts_with("@retry") {
        parse_retry(line).map(Some)
    } // ...
}

// 3. 解析函数
fn parse_retry(line: &str) -> ParseResult<Metadata> {
    let count_str = line.strip_prefix("@retry").unwrap().trim();
    let count = count_str.parse::<u32>()
        .map_err(|_| ParseError::InvalidMetadata {
            line: 0,
            message: "Invalid retry count".to_string(),
        })?;
    Ok(Metadata::Retry(count))
}

// 4. 应用
pub fn apply_metadata(metadata: &Metadata, target: &mut RequestMetadata) {
    match metadata {
        // ...
        Metadata::Retry(count) => {
            target.retry_count = Some(*count);
        }
    }
}

// 5. 测试
#[test]
fn test_parse_retry() {
    let result = parse_metadata("@retry 3").unwrap().unwrap();
    assert!(matches!(result, Metadata::Retry(3)));
}
```

---

**当前版本**: v1.0 (静态 Match)  
**参考文档**: 
- `doc/metadata_parser_trait_approach.md`
- `doc/metadata_parser_comparison.md`
