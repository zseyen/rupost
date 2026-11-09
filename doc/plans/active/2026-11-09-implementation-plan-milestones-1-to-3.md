# RuPost 演进实施方案与精确执行指南 (Milestones 1 ~ 3)

- **文档状态**: Under Revision（双子代理对抗审查完成——9 P0 / 13 P1 已吸收，见文末「评审修订记录」；修订条目与正文冲突时以修订为准）
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
    pub time: String, // duration_ms/1000 保留 3 位小数
    #[serde(rename = "failure", skip_serializing_if = "Option::is_none")]
    pub failure: Option<JUnitFailure>,
    #[serde(rename = "error", skip_serializing_if = "Option::is_none")]
    pub error: Option<JUnitError>,
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
    pub failure_type: String, // 恒为 "AssertionFailure"（请求级异常走 <error> 节点）
    #[serde(rename = "$text")]
    pub details: String,
}

/// 请求级异常（网络/DNS/超时）→ 独立 <error> 节点（JUnit 规范区分 failure/error，
/// Jenkins 红绿图标依赖此区分；@errors 只统计前者，恒 0 的缺陷已修正）
#[derive(Debug, Clone, Serialize)]
pub struct JUnitError {
    #[serde(rename = "@message")]
    pub message: String,
    #[serde(rename = "@type")]
    pub error_type: String, // 恒为 "RequestError"
    #[serde(rename = "$text")]
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
    context.insert("$body", body.clone()); // 评审 P0：body 是 &mut String，需 clone（impl Into<String> 无 From<&mut String>）
}

// 4. 最后求值 Headers（此时 Header 内的签名宏可安全引用 {{$body}} 和一致的时间戳）
for (_key, value) in &mut parsed.headers {
    *value = Self::resolve(value, context);
}

// 4.5 【评审 P0 补回】保留既有 user_agent 全局注入逻辑（resolver.rs:96-105 原文，
//     守护测试 test_resolve_parsed_request + tests/user_agent_test.rs 锁定——照原伪代码照抄会删功能）
// 5. 清理临时注入字段（【评审 P1 改判】改用请求级影子上下文方案：
//    let mut scoped = context.clone(); 冻结键与 $body 注入 scoped；求值全程走 scoped；
//    用完即弃——零清理、零泄漏（防 panic 时冻结值经 extract_changed_vars 扩散给并行依赖节点），
//    原文 context.remove 方案作废（VariableContext 无 remove API，且中途 panic 泄漏冻结值）
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
   cargo test --workspace   # 【评审 P0 勘正】--test integration_tests 不存在；
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

---

## 5. 评审修订记录（双子代理对抗审查，2026-11-09）

> **审查方式**：设计事实核查（方案 vs post-PR#13 代码库逐条比对）+ 对抗红队（变异推演/回归爆炸半径/画像场景交互）双代理并行。
> **总体结论**：可执行度 6/10——文件定位/API 名称/现状结构判断准确率约 85%，M1 基本可直接执行；M2/M3 存在 9 处照抄必错点（P0）。以下修订**与正文冲突时以本节为准**。

### 5.1 P0 修订（照原方案实现必错/必漏）

| # | 里程碑 | 缺陷 | 修订定案 |
| :- | :- | :- | :- |
| P0-1 | M3 | **parallel + fail_fast 下 teardown"Finally 保证"不可达**：`join_set.abort_all()`（parallel.rs:196）后被取消 worker 返回 `JoinError`，:201-206 `return Err` 提前退出，跳过 :210-217 变量合并（方案 C 节误诊为"合并缺失"——合并代码已存在） | `ParallelScheduler::run` 的 join 循环区分 `JoinError::is_cancelled()`（fail_fast 产物，debug 日志后 continue）与真实 panic；被 abort 任务补发 `TaskOutput{success:false}`；`run()` 在 fail_fast 时返回 `Ok(部分结果)`，Finally 挂在 execute_batch 的 Ok/Err 两路汇合点。Red 测试：parallel + fail_fast + 多文件（超并发度）下 teardown 仍执行且不返回 Err（注释 `is_cancelled` 分支必红） |
| P0-2 | M3 | **partition 破坏 `request_number` 原序号契约**：teardown 抽走后普通请求编号前移，破坏 `--only N`/`path#N`/JSON 报告/ai triage 的 `<file>#<n>` 定位（executor.rs:231 enumerate 语义） | partition 必须保留原始文件内序号（teardown 结果沿用原 request_number 归入原文件结果列表）。Red 测试：3 请求文件（#2 为 teardown）批量执行后第 3 条普通请求 `request_number == 3` |
| P0-3 | M2 | **`$random_int(100, 1)` 引入新 panic 面**：start > end 时 `random_range` panic——M2 使其从无害字面量变为 CI 崩溃 | 范围 start > end 自动交换 + 一次性 warning；任一参数解析失败该宏整体回退字面量原样并计入 warnings。Red 测试锁定 `{{$random_int(100, 1)}}`、`{{$random_int(abc, 5)}}`、`{{$random_string(0)}}` 均不得 panic |
| P0-4 | M2 | **resolve_parsed_request 伪代码丢失 User-Agent 注入**（resolver.rs:96-105），照抄删功能且 `test_resolve_parsed_request` + `tests/user_agent_test.rs` 变红 | 步骤 4 后补"4.5 保留 user_agent 全局注入"，两测试列入 M2 必保持绿回归清单 |
| P0-5 | M2 | **`context.insert("$body", body)` 编译不过**：body 为 `&mut String`，无 `From<&mut String>`；且 `VariableContext::remove` API 全库不存在，方案调用却未列任务 | `insert("$body", body.clone())`；新增 `VariableContext::remove` API 任务项。**已升级为影子上下文方案**（见 P1-7） |
| P0-6 | M1 | **quick-xml `$value` 用法错误**：文本内容特殊键是 `$text`，`$value` 产出非法元素名 | 结构体已修正为 `$text`（正文已改） |
| P0-7 | M3 | **门禁命令指向不存在的测试目标**：`--test integration_tests` 不存在 | 已勘正为 `cargo test --workspace`（正文已改） |
| P0-8 | M2 | **`\\{{` 误判转义**：`C:\\dir\\{{file}}` 中第二个 `\` 紧邻 `{{` 被吃掉且变量不解析 | 转义判定规则修订（`\\{{` 不算转义）+ 补 `\\{{var}}` 表驱动测试 |
| P0-9 | M3 | **未解析预检照字面实现会把全部 teardown 误判 skip**：`unresolved_placeholders` 契约是渲染后残留检测（resolver.rs:56-80），对未渲染请求调用恒命中 | 防呆必须"先 resolve 渲染副本 → 再检残留 → 通过后送 execute_one"，双次渲染幂等前提入 PRD |

### 5.2 P1 规格补充（实现歧义收口）

| # | 里程碑 | 补充规格 |
| :- | :- | :- |
| P1-1 | M1 | **CI 级验收补齐**：新增 `tests/junit_schema_test.rs`（对标 report_schema_test）：E2E 跑 `--report-file junit.xml` 断言 ①退出码与失败数一致 ②Jenkins junit XSD 可解析（`tests/fixtures/` 置 XSD）③`testsuite name/classname/time/skipped` 齐全；verify_features.sh 增第 14 步 |
| P1-2 | M1 | **skipped/failure/error 优先级**：skipped > failure > error；system_out = `Method URL / Status / warnings`，响应体片段仅失败用例附带（引用 response_body_snippet 现状——成功用例无 snippet） |
| P1-3 | M1 | **time 聚合公式**：testcase.time = duration_ms/1000 保留 3 位小数；testsuite.time = 用例和（保证可加和性）；testsuites.time 同理 |
| P1-4 | M1 | **dispatch 提示条件**：junit 落盘后纳入 dispatch.rs:81-83 的"已生成"提示（现仅匹配 Html，junit 静默写文件） |
| P1-5 | M2 | **零回归声明（最大缺口）**：Green 门禁必须跑全量——resolver.rs 9 单测、variable_integration 14 测、end_to_end（`{{ "username": "admin" }}` 字面量双花括号体——lookahead 遇 `"` 判非法的关键守护）、websocket_integration 8 测、adversarial_regression（跨行坏 JSON + 未定义警告）、mock_integration（matcher.rs 也走 resolve）、capture_integration（global. 前缀）、export_integration、generate_integration 12 测（T1 形态重放）、single_request、examples/run_all.sh。任何变红即语义破坏，禁止改测试适配实现 |
| P1-6 | M2 | **splitter 契约**：逗号分割（引号内豁免）→ 逐参 trim → 剥成对引号 → 再 trim；表驱动覆盖 `5, 10` / `"5","10"` / `'5',' 10'` / `"a,b"` / 空参（未定义 trim 序导致 `" 10"` parse 失败被 unwrap_or 静默改范围） |
| P1-7 | M2 | **影子上下文替代 remove**：`scoped = context.clone()` 冻结键+`__body` 注入 scoped，用完即弃——防 panic 泄漏冻结值经 extract_changed_vars（parallel.rs:302-313）扩散给依赖节点；`__body` 双下划线保留字 + parse_capture 拒绝 `__`/`$` 开头变量名（Red 锁定，防用户 `@capture $body` 命名空间冲突） |
| P1-8 | M2 | **三项行为变化显式声明**：① `\{{` 转义（新）② `{{ var }}` 空格容忍（从"警告未定义"变静默解析，警告面收窄）③ 嵌套 `{{a{{b}}}}` 求值语义改变。补锁定测试：`{{ variable }}`/`{{a}}` 单字符/`{{$x y}}` miss 返字面量/`{{中文}}` 保持字面量 |
| P1-9 | M3 | **变量防呆单次渲染**：resolve 一次后检查占位符，命中记 skipped；未命中用已渲染请求走 runner（或显式声明宏双求值限制）——检查所见必须与实际外发一致 |
| P1-10 | M3 | **teardown 语义矩阵**：①失败后剩余 teardown 继续执行，最终聚合失败（清理尽力而为）②`--only` 模式默认跳过全部 teardown（打印提示），仅显式选中时执行 ③`@skip` 标记的 teardown 豁免；partition 仅抽取未被 skip 的 teardown |
| P1-11 | M3 | **`TestResult.is_teardown` 字段补回**（上级路线图 6.2 明确要求，方案丢失）：JSON/JUnit/终端可区分；快照导出默认排除 teardown 结果，replay 遇到跳过并提示（防 DELETE 清理请求被重放） |
| P1-12 | M3 | **CORS 接线全链路**：`MockServer` trait（server.rs:6-7）、`run_server_from_file`（server.rs:157）、dispatch.rs:126、TUI serve_matcher（integrations.rs:136）+ 2 个测试调用方共 5 处签名变更；`Allow-Headers: *` 在 credentialed 请求下不展开（Fetch 规范）→ 预检改回显 `Access-Control-Request-Headers` 值；middleware 挂 fallback 之后；Red 测试：credentialed 预检必须 204 且回显请求头列表 |
| P1-13 | M3 | **CORS 路线裁决**：tower-http cors 已在依赖树（Cargo.toml:72，web/server.rs:103 已用 CorsLayer::permissive）——裁决：手写轻量中间件（需 mirror_request + 动态 Request-Headers 回显，tower-http 0.6 `AllowOrigin::mirror_request` 可达成，但 Allow-Headers 动态回显需自定义）vs 复用 tower-http——实现者按代码量裁决，理由入提交 |
| P1-14 | M1 | **ANSI 泄漏源勘正**：库内错误/断言/警告链路无 ANSI（error.rs:69、evaluator.rs:39-45 实证）——strip 是防御外部服务响应自带控制字符，非修复现存缺陷；剥离统一出口 = `to_xml_string` 序列化前对所有文本字段单点过 strip（含 @name/@message），另 strip-ansi-escapes 0.2 实际 API 签名与伪代码可能不符，Red 阶段先锁编译 |
| P1-15 | M1 | **与 --mask-secrets 关系声明**：junit 无条件复用 build_json_reports 脱敏（json.rs:56-138）；`--mask-secrets` 仅影响快照落盘——两条脱敏管线独立，文档声明 |

### 5.3 P2 建议（择机吸收）

1. **M2 独立成单独 PR**（波及 6 个运行面：ws_runner×6、http_runner、sse_runner、mock/matcher、exporter/preparer、TUI——风险最高），方案补估时（M1≈1 天、M2≈2-3 天、M3≈2 天，参照 T8 卡口径）；
2. `hex = "0.4"` 引入后收编 ws_runner.rs:331 私有 `mod hex`（DRY/SSOT）；
3. 台账联动全清单：CHANGELOG（M1 新参数/M3 CORS 默认变更）、README、runbook T8 卡翻正（archive/2026-q4/:108）、verify_features 新步骤（M1 junit/M3 teardown/CORS）、variables_and_environments.md（新宏全集+转义+参数语法）、testing_guide.md（@teardown 语义矩阵）、mock_server.md（CORS 默认开）、debug_guide（--report junit 行）、CLI help 文案（--report 现写死 "terminal, json 或 html"）；
4. quick-xml 上探 0.42（serde 契约一致）；"Rust 官方维护 strip-ansi-escapes"表述勘正（社区维护）；
5. `$hmac_sha256_b64` 或格式参数（移动端常 base64）；`$md5` 文档标注"仅遗留兼容"；`$hmac_sha256(key, data)` key 在前惯例文档明示；
6. 扫描器边界文档：引号内 `}}` 提前闭合不支持（`{{$hmac(k, "}}")}}`）；chars Vec<char> 4 倍内存放大（后续字节级优化，不阻塞）；
7. CORS 细节：`Vary: Origin` 附加；`Access-Control-Max-Age` 可选；OPTIONS 204 短路屏蔽用户显式 OPTIONS mock（--no-cors 可绕，文档标注）；
8. teardown 收尾：serial Finally 后补 `persist_cookies`（batch.rs:80 对齐）；teardown-only 文件与 `@depends-on`、未标 `@test` 文件的 gating 交互一句话声明；
9. PRD §八"verify 12/12.5 承重墙"表述勘正（实测不依赖占位跳过分支）。

### 5.4 修订后执行顺序

```
M1（~1 天）：依赖引入 → Red（format 表驱动 + junit 单测 + junit_schema_test）→ Green → verify 步骤 14
M3（~2 天）：P0-1 parallel 容忍改造 → P0-2 partition 序号保留 → @teardown 语法 → 防呆单次渲染
            → --only/skip 语义 → is_teardown 字段 → CORS 路线裁决后实现 → verify 步骤 15/16
M2（~2-3 天，独立 PR + 独立全量门禁）：影子上下文 → 平衡扫描器 → 密码学宏 → P0-3/P0-8 防御
            → 13 项回归清单全绿 → verify 步骤 17
收尾：全套验收四命令 → 台账（5.3 清单）→ progress_summary/checkpoint
```

**顺序理由**：M2 风险最高（波及 6 运行面）且独立成 PR，移至最后单独评审；M1 最小先行建 CI 闭环；M3 的 parallel 容忍改造是独立于 resolver 的调度层工作，先于 M2 降低叠加风险。
