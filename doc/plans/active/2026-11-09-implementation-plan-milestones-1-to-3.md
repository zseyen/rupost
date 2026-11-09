# RuPost 演进实施方案与精确执行指南 (Milestones 1 ~ 3)

- **文档状态**: Approved / Ready for Execution
- **创建日期**: 2026-11-09
- **归档路径**: `doc/plans/active/2026-11-09-implementation-plan-milestones-1-to-3.md`
- **目标执行者**: AI 研发智能体（含 Flash 级模型）/ 人类开发者
- **关联上级计划**: [`doc/plans/active/2026-11-09-full-scenario-architecture-review-and-roadmap.md`](file:///Users/zsyzzx/project/rust/rupost/doc/plans/active/2026-11-09-full-scenario-architecture-review-and-roadmap.md)
- **事实来源**: [`doc/plans/progress_summary.md`](file:///Users/zsyzzx/project/rust/rupost/doc/plans/progress_summary.md)

---

## 0. 执行总纲与纪律铁律 (Execution Directives)

接手本方案的执行智能体（Agent）必须严格遵守以下工程规范：
1. **Clean Architecture 分层约束**：
   - 表现层/适配器层（`src/cli/`, `src/report/`）负责参数解析、格式转换与写盘；
   - 领域/用例层（`src/runner/`, `src/variable/`, `src/parser/`）负责调度、生命周期保证与宏求值；业务逻辑严禁向上泄露至 CLI 派发分支。
2. **TDD 驱动开发（Red-Green-Refactor）**：
   - 每个子任务必须**先写失败单测（Red）**，单测确认真实报错后，再极简实现**使单测变绿（Green）**，最后重构清理孤儿代码（Refactor）；
   - **反假守护检验**：注释或反转生产代码，守护测试必须确定性变红，禁止假断言。
3. **Jujutsu (jj) 原子化版本提交**：
   - 每个小任务独立提交一次，提交描述必须规范化（Conventional Commits），格式如：
     `jj describe -m "feat(report): implement junit xml report generator with ansi stripping" && jj new`
4. **三层回归门禁**：
   - 阶段交付前必须依次通过：
     1. `cargo clippy --workspace --all-targets -- -D warnings`
     2. `cargo test --workspace`
     3. `bash tests/verify_features.sh`
     4. `bash examples/run_all.sh`

---

## Milestone 1: CI 闭环与工业级 JUnit XML 测试报告

### 1.1 需求目标
支持 `--report junit` 及 `--report-file <PATH>`（根据 `.xml` 扩展名推断），将测试结果转换为 100% 符合 Jenkins/GitLab/GitHub Actions 标准规格的 JUnit XML 报告。
- 必须基于 `build_json_reports` 的脱敏结果生成，**严禁明文暴露 URL 密钥与响应敏感字段**；
- 必须剥离终端 ANSI 彩色转义码（`\x1b[31m` 等），防止 XML 解析器抛出 `SAXParseException`。

### 1.2 依赖变更 (`Cargo.toml`)
在 `Cargo.toml` 的 `[dependencies]` 下添加：
```toml
# 纯 Rust XML 序列化（轻量无 C 依赖，工业级转义）
quick-xml = { version = "0.37", features = ["serialize"] }
# 字节级安全剥离终端 ANSI 控制字符（Rust 官方维护）
strip-ansi-escapes = "0.2"
```

### 1.3 核心实现细节与文件归宿

#### A. 新建 `src/report/junit.rs`
构建符合 JUnit 规范的强类型结构体：
```rust
use crate::report::json::JsonFileReport;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename = "testsuites")]
pub struct JUnitTestSuites {
    #[serde(rename = "@tests")]
    pub tests: usize,
    #[serde(rename = "@failures")]
    pub failures: usize,
    #[serde(rename = "@errors")]
    pub errors: usize,
    #[serde(rename = "@skipped")]
    pub skipped: usize,
    #[serde(rename = "@time")]
    pub time: String, // 格式化为保留 3 位小数的秒数，如 "0.125"
    #[serde(rename = "testsuite")]
    pub suites: Vec<JUnitTestSuite>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JUnitTestSuite {
    #[serde(rename = "@name")]
    pub name: String, // POSIX 归一化后的测试文件路径
    #[serde(rename = "@tests")]
    pub tests: usize,
    #[serde(rename = "@failures")]
    pub failures: usize,
    #[serde(rename = "@errors")]
    pub errors: usize,
    #[serde(rename = "@skipped")]
    pub skipped: usize,
    #[serde(rename = "@time")]
    pub time: String,
    #[serde(rename = "testcase", default)]
    pub testcases: Vec<JUnitTestCase>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JUnitTestCase {
    #[serde(rename = "@name")]
    pub name: String, // 形如 "#1 (auth)" 或 "#2"
    #[serde(rename = "@classname")]
    pub classname: String, // 文件路径
    #[serde(rename = "@time")]
    pub time: String,
    #[serde(rename = "failure", skip_serializing_if = "Option::is_none")]
    pub failure: Option<JUnitFailure>,
    #[serde(rename = "skipped", skip_serializing_if = "Option::is_none")]
    pub skipped: Option<JUnitSkipped>,
    #[serde(rename = "system-out", skip_serializing_if = "Option::is_none")]
    pub system_out: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JUnitFailure {
    #[serde(rename = "@message")]
    pub message: String,
    #[serde(rename = "@type")]
    pub failure_type: String, // "AssertionFailure" | "RequestError"
    #[serde(rename = "$value")]
    pub details: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct JUnitSkipped {
    #[serde(rename = "@message", skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}
```

**转换与清洗方法**：
```rust
impl JUnitTestSuites {
    /// 从已完成统一脱敏的 JsonFileReport 列表构建（SSOT 脱敏复用）
    pub fn from_json_file_reports(reports: &[JsonFileReport]) -> Self {
        // 1. 遍历 reports，汇总 tests, failures, errors, skipped, time
        // 2. 将每个 JsonTestReport 转换为 JUnitTestCase:
        //    - 若 r.skipped 为 true -> 填充 skipped 节点
        //    - 若 !r.success -> 填充 failure 节点（包含断言未通过的 expected/actual 或 r.error）
        //    - 拼接 system_out（包含 URL、Status、脱敏响应体片段与 warnings）
        // 3. 所有写入 failure.details 和 system_out 的字符串，必须统一经过：
        //    let clean_text = String::from_utf8_lossy(&strip_ansi_escapes::strip(raw_text)).to_string();
    }

    /// 序列化为符合 XML 规范的字符串
    pub fn to_xml_string(&self) -> crate::Result<String> {
        let mut buffer = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        let xml = quick_xml::se::to_string(self)
            .map_err(|e| crate::RupostError::Other(format!("JUnit XML 序列化失败: {e}")))?;
        buffer.push_str(&xml);
        Ok(buffer)
    }
}
```

#### B. 扩展 `src/report/mod.rs`
1. 扩展 `ReportFileFormat` 枚举：
   ```rust
   pub enum ReportFileFormat {
       Html,
       Json,
       Junit,
   }
   ```
2. 更新 `determine_report_format`：
   ```rust
   pub fn determine_report_format(report: &str, report_file: &str) -> ReportFileFormat {
       let ext_lower = report_file.to_ascii_lowercase();
       let is_html_ext = ext_lower.ends_with(".html") || ext_lower.ends_with(".htm");
       let is_xml_ext = ext_lower.ends_with(".xml");

       if report.eq_ignore_ascii_case("html") {
           ReportFileFormat::Html
       } else if report.eq_ignore_ascii_case("json") {
           ReportFileFormat::Json
       } else if report.eq_ignore_ascii_case("junit") || report.eq_ignore_ascii_case("xml") {
           ReportFileFormat::Junit
       } else if is_html_ext {
           ReportFileFormat::Html
       } else if is_xml_ext {
           ReportFileFormat::Junit
       } else {
           ReportFileFormat::Json // 保持既有单测锁定的兜底
       }
   }
   ```
3. 更新 `write_report_files`：
   - 增加 `--report junit` 且无 `report_file` 时默认落盘 `rupost-junit.xml` 的逻辑；
   - 在 `match format` 中增加 `ReportFileFormat::Junit` 分支：
     ```rust
     ReportFileFormat::Junit => {
         let json_reports = json::build_json_reports(batch_results);
         let suites = junit::JUnitTestSuites::from_json_file_reports(&json_reports);
         let xml_content = suites.to_xml_string()?;
         std::fs::write(&path, xml_content)?;
     }
     ```

### 1.4 Milestone 1 TDD 验证步骤
1. **Red**: 在 `src/report/mod.rs` 的 `tests` 模块追加 `determine_report_format` 的 JUnit 表驱动测试用例：
   - `("junit", "out.json", ReportFileFormat::Junit)`
   - `("terminal", "test.xml", ReportFileFormat::Junit)`
   - `("terminal", "TEST.XML", ReportFileFormat::Junit)`
2. **Red**: 在 `src/report/junit.rs` 编写单元测试：
   - 测试含 ANSI 颜色转义字符（`\x1b[31mFAIL\x1b[0m`）的响应文本，验证生成的 XML 中 ANSI 控制符被彻底剥离；
   - 验证敏感 Token 在生成的 XML 中已被脱敏（带有 `***` 掩码）；
   - 验证包含 `&`, `<`, `>`, `"`, `'` 的文本在 XML 中被合规转义。
3. **Green**: 实现上述逻辑，使单测 100% 通过。
4. **JJ 提交**:
   ```bash
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --package rupost --lib report
   jj describe -m "feat(report): implement junit xml report with ansi stripping and redaction reuse"
   jj new
   ```

---

## Milestone 2: 零依赖平衡词法扫描器与动态带参宏 (移动端加签支持)

### 2.1 需求目标
1. 替换脆弱的正则替换，手写单行括号平衡词法扫描器（Balanced Lexer），单次线性遍历，支持任意深度递归嵌套展开；
2. 语法免疫：单行约束 + 标识符前瞻守卫（必须为 `$` 或字母开头），支持 `\{{` 转义，彻底免疫 GraphQL 与压缩 JSON `{{` 误伤；
3. 拓扑与时钟快照：单请求周期内冻结时间基准；调整求值顺序为 **先求值 URL/Body -> 注入只读上下文 `$body` -> 再求值 Headers**，彻底解决移动端加签时序死锁；
4. 引入纯 Rust 轻量哈希库，闭环 `$hmac_sha256`、`$sha256`、`$md5` 以及 Faker 宏 `$random_int(min, max)`、`$random_string(len)`、`$random_email`。

### 2.2 依赖变更 (`Cargo.toml`)
在 `[dependencies]` 下添加（纯 Rust 微库，零 C 绑定，全平台通用）：
```toml
sha2 = "0.10"
hmac = "0.12"
md-5 = "0.10"
hex = "0.4"
```

### 2.3 核心实现细节与文件归宿

#### A. 重构 `src/variable/resolver.rs`

**1. 引号感知的参数分割器（Quote-Aware Argument Splitter）**
编写纯函数 `split_macro_arguments(args_str: &str) -> Vec<String>`：
- 正确识别被双引号 `"` 或单引号 `'` 包裹的参数；
- 引号内部的逗号 `,` 不触发分割；
- 剥离外层包裹的引号，保留内部字面量。

**2. 平衡词法扫描器核心 `substitute_balanced`**
```rust
pub fn substitute_balanced(text: &str, context: &VariableContext, depth: usize) -> String {
    if depth > 10 || !text.contains("{{") {
        return text.to_string();
    }

    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // 转义逃生舱：\{{ -> {{
        if chars[i] == '\\' && i + 2 < len && chars[i+1] == '{' && chars[i+2] == '{' {
            out.push_str("{{");
            i += 3;
            continue;
        }

        // 检测占位符起点 "{{"
        if i + 1 < len && chars[i] == '{' && chars[i+1] == '{' {
            // 前瞻检查：非换行，且后继首个有效字符必须符合 [$a-zA-Z_]
            let lookahead_idx = i + 2;
            let mut valid_start = false;
            let mut peek = lookahead_idx;
            while peek < len && chars[peek] == ' ' { peek += 1; }
            if peek < len && (chars[peek] == '$' || chars[peek].is_ascii_alphabetic() || chars[peek] == '_') {
                valid_start = true;
            }

            if valid_start {
                let start = i + 2;
                let mut nested = 0;
                let mut end = None;
                let mut j = start;

                while j + 1 < len {
                    if chars[j] == '\n' { break; } // 单行约束：跨行立即作废
                    if chars[j] == '{' && chars[j+1] == '{' {
                        nested += 1;
                        j += 2;
                    } else if chars[j] == '}' && chars[j+1] == '}' {
                        if nested == 0 {
                            end = Some(j);
                            break;
                        } else {
                            nested -= 1;
                            j += 2;
                        }
                    } else {
                        j += 1;
                    }
                }

                if let Some(end_idx) = end {
                    let raw_expr: String = chars[start..end_idx].iter().collect();
                    let trimmed_expr = raw_expr.trim();
                    // 递归展开内层占位符（如参数中包含内层宏）
                    let expanded_expr = substitute_balanced(trimmed_expr, context, depth + 1);
                    // 求值表达式
                    let val = evaluate_expression(&expanded_expr, context);
                    out.push_str(&val);
                    i = end_idx + 2;
                    continue;
                }
            }
        }

        out.push(chars[i]);
        i += 1;
    }
    out
}
```

**3. 宏分发与求值器 `evaluate_expression`**
```rust
fn evaluate_expression(expr: &str, context: &VariableContext) -> String {
    // 1. 带参函数宏识别：形如 $fn_name(...)
    if expr.starts_with('$') && expr.contains('(') && expr.ends_with(')') {
        evaluate_macro_call(expr, context)
    } else {
        // 2. 无参动态宏与普通变量识别
        match expr {
            "$uuid" => uuid::Uuid::new_v4().to_string(),
            "$timestamp" => context.get("__request_timestamp").unwrap_or_else(|| chrono::Utc::now().timestamp().to_string()),
            "$timestamp_ms" => context.get("__request_timestamp_ms").unwrap_or_else(|| chrono::Utc::now().timestamp_millis().to_string()),
            "$timestamp_iso" => context.get("__request_timestamp_iso").unwrap_or_else(|| chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
            "$random_int" => rand::rng().random_range(1..=10000).to_string(),
            "$random_email" => format!("user_{}@example.com", generate_random_alphanumeric(6)),
            "$random_string" => generate_random_alphanumeric(8),
            _ => context.get(expr).unwrap_or_else(|| format!("{{{{{}}}}}", expr)),
        }
    }
}

fn evaluate_macro_call(expr: &str, context: &VariableContext) -> String {
    let (fn_name, args_raw) = match expr.split_once('(') {
        Some((f, rest)) => (f.trim(), rest.trim_end_matches(')').trim()),
        None => return format!("{{{{{}}}}}", expr),
    };

    let args = split_macro_arguments(args_raw);

    match fn_name {
        "$hmac_sha256" if args.len() >= 2 => {
            crypto_hmac_sha256(&args[0], &args[1])
        }
        "$sha256" if !args.is_empty() => {
            crypto_sha256(&args[0])
        }
        "$md5" if !args.is_empty() => {
            crypto_md5(&args[0])
        }
        "$random_int" if args.len() >= 2 => {
            let start = args[0].parse::<i64>().unwrap_or(1);
            let end = args[1].parse::<i64>().unwrap_or(100);
            rand::rng().random_range(start..=end).to_string()
        }
        "$random_string" if !args.is_empty() => {
            let len = args[0].parse::<usize>().unwrap_or(8);
            generate_random_alphanumeric(len)
        }
        _ => format!("{{{{{}}}}}", expr), // 未知宏原样保留，供未定义警告捕获
    }
}
```

**4. 字段拓扑求值与单请求时钟冻结 (`resolve_parsed_request`)**
在 `resolve_parsed_request(parsed: &mut ParsedRequest, context: &mut VariableContext)` 中：
```rust
// 1. 冻结单请求时间基准
let now = chrono::Utc::now();
context.insert("__request_timestamp", &now.timestamp().to_string());
context.insert("__request_timestamp_ms", &now.timestamp_millis().to_string());
context.insert("__request_timestamp_iso", &now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));

// 2. 优先求值 URL
parsed.url = Self::resolve(&parsed.url, context);

// 3. 优先求值 Body，并将求值后的 Body 注入临时上下文 $body
if let Some(body) = &mut parsed.body {
    *body = Self::resolve(body, context);
    context.insert("$body", body);
}

// 4. 最后求值 Headers（此时 Header 内的签名宏可安全引用 {{$body}} 和一致的时间戳）
for (_key, value) in &mut parsed.headers {
    *value = Self::resolve(value, context);
}

// 5. 清理临时注入字段
context.remove("$body");
context.remove("__request_timestamp");
context.remove("__request_timestamp_ms");
context.remove("__request_timestamp_iso");
```

### 2.4 Milestone 2 TDD 验证步骤
1. **Red**: 编写测试用例锁定带参加签与内联嵌套展开：
   `VariableResolver::resolve("Sign: {{$hmac_sha256(secret, \"{{$timestamp_ms}}:{{$body}}\")}}", &ctx)`
2. **Red**: 编写测试用例验证时钟一致性：同一个请求内多个 `{{$timestamp_ms}}` 必须字符串完全相等；
3. **Red**: 编写测试用例验证 GraphQL/JSON 免疫性：`query { viewer { id } }` 与 `\{{escaped}}` 不得被篡改；
4. **Green**: 实现扫描器与密码学哈希，让单测 100% 变绿；
5. **JJ 提交**:
   ```bash
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --package rupost --lib variable
   jj describe -m "feat(variable): balanced recursive scanner with crypto macros and request-scoped body injection"
   jj new
   ```

---

## Milestone 3: `@teardown` 确定性生命周期与 Mock 智能 CORS

### 3.1 需求目标
1. 语法支持：请求块前声明 `@teardown` 元数据；
2. 调度保障：`BatchExecutor` 在常规执行结束后（或遭遇失败/`--fail-fast` 时），在 `Finally` 阶段按**已执行文件的逆拓扑序 + 文件内自然书写序**确定性执行 `@teardown`；
3. 变量缺失防呆保护：若前置步骤失败导致 `@teardown` 依赖的变量未捕获，自动标记为 `Skipped` 并输出警告，杜绝脏请求发往服务端；
4. `rupost mock` 智能 CORS：默认支持预检 `OPTIONS` 秒级响应 204，动态回显 `Origin` 并设置 `Access-Control-Allow-Credentials: true`，提供 `--no-cors` CLI 选项。

### 3.2 核心实现细节与文件归宿

#### A. 语法解析层 (`src/parser/`)
1. 在 `src/parser/types.rs` 的 `RequestMetadata` 增加字段：
   ```rust
   pub struct RequestMetadata {
       // ...
       pub is_teardown: bool,
   }
   ```
2. 在 `src/parser/metadata.rs` 的 `parse_metadata` 注册指令：
   ```rust
   "@teardown" => Ok(Some(Metadata::Teardown)),
   ```
   并在 `apply_metadata` 中赋值 `target.is_teardown = true;`。

#### B. 批处理引擎改造 (`src/runner/engine/batch.rs`)
1. 区分常规测试与清理步骤：
   ```rust
   impl BatchExecutor {
       /// 将文件切分为普通请求列表与 teardown 请求列表
       fn partition_file_requests(file: ParsedFile) -> (ParsedFile, Vec<ParsedRequest>) {
           let mut normal_requests = Vec::new();
           let mut teardowns = Vec::new();
           for req in file.requests {
               if req.metadata.is_teardown {
                   teardowns.push(req);
               } else {
                   normal_requests.push(req);
               }
           }
           let mut normal_file = file.clone();
           normal_file.requests = normal_requests;
           (normal_file, teardowns)
       }
   }
   ```
2. 在 `execute_batch` 中挂载 `Finally` 清理守卫：
   - 记录已实际开始执行的文件列表；
   - 无论正常完成还是因 `has_failure && request.fail_fast` 提前中断：
   - 依次按已启动文件的**逆拓扑顺序**取出各文件的 `teardowns` 队列；
   - 检查 `teardown` 请求是否存在未解析变量（调用 `VariableResolver::unresolved_placeholders`）：
     - 若存在未解析占位符：记录为 `TestResult::skipped(...)`，附带告警信息；
     - 若变量齐全：调用 `executor.execute_one` 发送清理请求，并记录其结果；
   - 清理结果若发生错误（!success），将 `batch_results` 整体标记为失败。

#### C. 并行调度退出守护 (`src/runner/engine/parallel.rs`)
在 `ParallelScheduler::run` 中，当触发 `fail_fast` 调用 `join_set.abort_all()` 后：
- 确保从已有完成节点的通道收集已产出的 `captured_vars` 合并回 `self.context`，保证主执行器获取到清理所需的有效 ID。

#### D. Mock 智能 CORS (`src/mock/server/axum_adapter.rs`)
修改 `axum_adapter`，提供智能预检与 Header 注入中间件：
```rust
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};

pub async fn cors_middleware(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let origin = req.headers().get("origin").cloned();
    let is_options = req.method() == Method::OPTIONS;

    // 预检请求直接应答 204 No Content
    if is_options {
        let mut resp = StatusCode::NO_CONTENT.into_response();
        inject_cors_headers(resp.headers_mut(), origin.as_ref());
        return resp;
    }

    let mut resp = next.run(req).await;
    inject_cors_headers(resp.headers_mut(), origin.as_ref());
    resp
}

fn inject_cors_headers(headers: &mut HeaderMap, origin: Option<&HeaderValue>) {
    if let Some(orig) = origin {
        headers.insert("Access-Control-Allow-Origin", orig.clone());
        headers.insert("Access-Control-Allow-Credentials", HeaderValue::from_static("true"));
    } else {
        headers.insert("Access-Control-Allow-Origin", HeaderValue::from_static("*"));
    }
    headers.insert(
        "Access-Control-Allow-Methods",
        HeaderValue::from_static("GET, POST, PUT, DELETE, PATCH, OPTIONS, HEAD"),
    );
    headers.insert(
        "Access-Control-Allow-Headers",
        HeaderValue::from_static("*"),
    );
}
```
CLI 在 `src/cli/commands/mock.rs` 增加 `--no-cors` 参数，默认 `enable_cors = !args.no_cors`。

### 3.3 Milestone 3 TDD 验证步骤
1. **Red**: 编写集成测试用例：定义包含创建与 `@teardown` 的 `.http` 文件，故意让主断言失败，断言验证 `@teardown` 依然被执行；
2. **Red**: 编写测试用例验证缺失变量防呆：当创建请求失败且未产生 `{{user_id}}` 时，`@teardown` 请求被标记为 `Skipped` 而非外发垃圾报文；
3. **Red**: 使用 `reqwest` 发起携带 `Origin: http://localhost:3000` 的 `OPTIONS` 预检请求，断言返回 204 且包含 `Access-Control-Allow-Credentials: true`；
4. **Green**: 实现上述执行器调度与中间件，全量单测通过；
5. **JJ 提交**:
   ```bash
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --package rupost --test integration_tests
   jj describe -m "feat(runner): deterministic @teardown lifecycle guarantee and mock smart cors echo"
   jj new
   ```

---

## 4. 全量回归与最终验收清单

三部分全部完成后，在根目录执行全套验收命令：
```bash
# 1. 静态格式与代码规范
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings

# 2. 全量单元测试与集成测试
cargo test --workspace

# 3. 既有特性冒烟回归脚本
bash tests/verify_features.sh

# 4. 全场景实例套件验证
bash examples/run_all.sh
```
验收全部 PASS 后，同步更新：
- [`doc/plans/progress_summary.md`](file:///Users/zsyzzx/project/rust/rupost/doc/plans/progress_summary.md)
- `checkpoint.md`

本方案指令明确、签名完备、边界严密，接手智能体可直接从 **Milestone 1 的 1.2 节依赖引入** 开始分步执行。
