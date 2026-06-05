# TestReporter 与 ResponseFormatter 联动设计

## 问题分析

### 现有能力
- ✅ `ResponseFormatter` 已实现：
  - Compact 模式：简洁输出（状态码 + 耗时 + Body摘要）
  - Verbose 模式：详细输出（状态码 + Headers + 完整Body）
  - JSON 格式化
  - 彩色输出

### 用户需求
1. 批量测试时：默认简洁输出（只看状态码和耗时）
2. 需要调试时：查看完整响应（Headers + Body）
3. 某个请求失败时：自动显示详细错误信息

---

## 优化设计方案

### 方案：TestReporter 复用 ResponseFormatter

#### 1. 测试结果结构增强

```rust
// src/runner/types.rs

use crate::http::Response;

pub struct TestResult {
    // ... 现有字段 ...
    
    /// 完整的 HTTP 响应（用于详细输出）
    pub response: Option<Response>,
}
```

#### 2. TestReporter 集成 ResponseFormatter

```rust
// src/runner/reporter.rs

use crate::runner::types::{TestResult, TestSummary};
use crate::utils::{ResponseFormat, ResponseFormatter};
use colored::Colorize;

pub struct TestReporter {
    verbose: bool,
    formatter: ResponseFormatter,
}

impl TestReporter {
    pub fn new(verbose: bool) -> Self {
        let format = if verbose {
            ResponseFormat::Verbose
        } else {
            ResponseFormat::Compact
        };
        
        Self {
            verbose,
            formatter: ResponseFormatter::new(format),
        }
    }
    
    /// 打印单个测试结果
    pub fn print_result(&self, result: &TestResult) {
        // 1. 打印测试状态行
        if result.success {
            println!(
                " {} [{}] {} {} ({}ms)",
                "✓".green(),
                result.request_number,
                result.method.cyan(),
                result.url,
                result.duration.as_millis()
            );
        } else {
            println!(
                " {} [{}] {} {} ({}ms)",
                "✗".red(),
                result.request_number,
                result.method.cyan(),
                result.url,
                result.duration.as_millis()
            );
        }
        
        // 2. 如果是 verbose 模式，或者失败了，显示详细信息
        if self.verbose || !result.success {
            if let Some(response) = &result.response {
                // 复用 ResponseFormatter 格式化响应
                match self.formatter.format(response) {
                    Ok(formatted) => {
                        // 缩进显示
                        for line in formatted.lines() {
                            println!("   {}", line);
                        }
                    }
                    Err(e) => {
                        println!("   {}: Failed to format response: {}", 
                                 "Warning".yellow(), e);
                    }
                }
                println!(); // 空行分隔
            }
        }
        
        // 3. 如果有错误消息（转换或网络错误），显示错误
        if let Some(error) = &result.error {
            println!("   {}: {}", "Error".red().bold(), error);
            println!();
        }
    }
    
    // ... 其他方法保持不变 ...
}

impl Default for TestReporter {
    fn default() -> Self {
        Self::new(false)
    }
}
```

---

## 使用场景示例

### 场景 1: 默认模式（简洁）

```bash
$ rupost test api.http

Running 4 requests from api.http...

 ✓ [1/4] GET https://api.example.com/users (234ms)
 ✓ [2/4] POST https://api.example.com/users (456ms)
 ✗ [3/4] GET https://api.example.com/invalid (123ms)
   Error: Request failed: 404 Not Found
 ✓ [4/4] DELETE https://api.example.com/users/1 (189ms)

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Summary
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Tests: 3 passed, 1 failed, 4 total
  Duration: 1.002s
```

**注意**：失败的请求[3]自动显示了错误信息

---

### 场景 2: Verbose 模式（详细）

```bash
$ rupost test api.http --verbose

Running 4 requests from api.http...

 ✓ [1/4] GET https://api.example.com/users (234ms)
   HTTP 200 OK
   Time: 234ms
   
   Headers:
      content-type: application/json
      content-length: 145
   
   Body:
   {
     "users": [
       {"id": 1, "name": "Alice"},
       {"id": 2, "name": "Bob"}
     ]
   }

 ✓ [2/4] POST https://api.example.com/users (456ms)
   HTTP 201 Created
   Time: 456ms
   
   Headers:
      content-type: application/json
      location: /users/3
   
   Body:
   {
     "id": 3,
     "name": "Charlie",
     "created_at": "2024-01-22T12:00:00Z"
   }

 ✗ [3/4] GET https://api.example.com/invalid (123ms)
   HTTP 404 Not Found
   Time: 123ms
   
   Headers:
      content-type: application/json
   
   Body:
   {
     "error": "Resource not found"
   }

 ✓ [4/4] DELETE https://api.example.com/users/1 (189ms)
   HTTP 204 No Content
   Time: 189ms

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Summary
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Tests: 3 passed, 1 failed, 4 total
  Duration: 1.002s
```

---

## CLI 参数设计

### 添加 --verbose 选项

```rust
#[derive(Subcommand)]
enum Commands {
    /// Run requests from a file
    Test {
        /// Path to the .http file
        file: String,
        
        /// Show detailed request/response information
        #[arg(short, long)]
        verbose: bool,
    },
}
```

### 使用方式

```bash
# 默认模式（简洁）
rupost test api.http

# 详细模式（显示完整响应）
rupost test api.http --verbose
rupost test api.http -v
```

---

## 优势总结

### ✅ 代码复用
- TestReporter 复用 ResponseFormatter 的所有格式化能力
- 减少重复代码
- 保持一致的输出风格

### ✅ 智能默认
- 默认模式：简洁，适合 CI/CD
- 失败自动详细：失败的请求自动显示详细信息
- Verbose 模式：完整信息，适合调试

### ✅ 灵活控制
- 用户可以通过 `--verbose` 控制输出详细度
- 未来可扩展更多选项（如 `--no-color`、`--json`）

### ✅ 最佳实践
- 符合常见测试框架的输出模式（Jest、pytest 等）
- 既适合人类阅读，也适合 CI 系统解析

---

## 实施优先级

1. **P0（本次实现）**:
   - TestResult 保存完整 Response
   - TestReporter 集成 ResponseFormatter
   - 失败请求自动显示详细信息
   - 添加 `--verbose` 选项

2. **P1（后续优化）**:
   - 添加 `--no-color` 选项（CI 环境）
   - 添加 `--json` 输出格式（机器可读）
   - 添加 `--quiet` 选项（只显示摘要）

---

## 文件变更总结

| 文件 | 变更类型 | 说明 |
|------|---------|------|
| `runner/types.rs` | 修改 | 添加 `response: Option<Response>` 字段 |
| `runner/reporter.rs` | 增强 | 集成 ResponseFormatter |
| `runner/executor.rs` | 修改 | 保存完整 Response 到 TestResult |
| `main.rs` | 修改 | 添加 `--verbose` 参数 |

---

**这个设计完美复用了现有能力，同时提供了灵活的控制选项！** 🎯
