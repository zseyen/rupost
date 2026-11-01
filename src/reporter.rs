use crate::runner::types::{TestResult, TestSummary};
use crate::utils::{ResponseFormat, ResponseFormatter};
use colored::Colorize;
use serde::{Deserialize, Serialize};

/// 单个文件的 JSON 测试报告（强类型序列化与反序列化契约，供 `ai triage` 消费）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonFileReport {
    pub file_path: String,
    pub results: Vec<JsonTestReport>,
}

/// 单个请求的 JSON 测试报告
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonTestReport {
    pub request_number: usize,
    pub name: Option<String>,
    pub method: String,
    pub url: String,
    pub status: Option<u16>,
    pub duration_ms: u64,
    pub success: bool,
    pub error: Option<String>,
    pub skipped: bool,
    pub assertions: Vec<JsonAssertionReport>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub diagnose_report: Option<crate::http::DiagnosticsReport>,
    /// 仅在失败请求 (!success) 且存在响应体时记录脱敏 + 512B 截断后的响应片段
    #[serde(default)]
    pub response_body_snippet: Option<String>,
}

/// 单条断言的 JSON 报告（保留 expected/actual/stream_event_index 供 E2/E5 分诊）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonAssertionReport {
    pub raw: String,
    pub passed: bool,
    pub message: Option<String>,
    #[serde(default)]
    pub expected: Option<String>,
    #[serde(default)]
    pub actual: Option<String>,
    #[serde(default)]
    pub stream_event_index: Option<usize>,
}

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
        // 跳过的测试
        if result.skipped {
            let name_part = if let Some(ref name) = result.name {
                format!(" {} -", name)
            } else {
                String::new()
            };
            println!(
                " {} [{}]{} {} {} {}",
                "⊘".dimmed(),
                result.request_number,
                name_part,
                result.method.cyan(),
                result.url,
                "(skipped)".dimmed()
            );
            return;
        }

        // 成功或失败的测试
        let symbol = if result.success { "✓" } else { "✗" };
        let color = if result.success { "green" } else { "red" };

        let name_part = if let Some(ref name) = result.name {
            format!(" {} -", name)
        } else {
            String::new()
        };

        println!(
            " {} [{}]{} {} {} ({}ms)",
            symbol.color(color),
            result.request_number,
            name_part,
            result.method.cyan(),
            result.url,
            result.duration.as_millis()
        );

        // 如果是 verbose 模式，或者失败了，显示详细信息
        if (self.verbose || !result.success)
            && let Some(response) = &result.response
        {
            // 复用 ResponseFormatter 格式化响应
            match self.formatter.format(response) {
                Ok(formatted) => {
                    // 缩进显示
                    for line in formatted.lines() {
                        println!("   {}", line);
                    }
                }
                Err(e) => {
                    println!(
                        "   {}: Failed to format response: {}",
                        "Warning".yellow(),
                        e
                    );
                }
            }
            println!(); // 空行分隔
        }

        // 如果有错误消息（转换或网络错误），显示错误
        if let Some(error) = &result.error {
            println!("   {}: {}", "Error".red().bold(), error);
            println!();
        }

        // 显示断言结果
        if !result.assertions.is_empty() {
            println!("   Assertions:");
            for assertion in &result.assertions {
                if assertion.passed {
                    println!("     {} {}", "✓".green(), assertion.raw);
                } else {
                    println!("     {} {}", "✗".red(), assertion.raw);
                    if let Some(msg) = &assertion.message {
                        println!("       {}", msg.red());
                    }
                }
            }
            println!();
        }

        // 显示非致命告警（capture 失败、未定义变量等）——必须可见，避免静默假通过
        if !result.warnings.is_empty() {
            println!("   {}:", "Warnings".yellow().bold());
            for warning in &result.warnings {
                println!("     {} {}", "!".yellow(), warning.yellow());
            }
            println!();
        }

        // 显示网络时序与诊断报告
        if let Some(ref report) = result.diagnose_report {
            crate::http::print_diagnose_report(report);
        } else if let Some(ref timing) = result.timing {
            self.print_timing(timing);
        }
    }

    fn print_timing(&self, timing: &crate::http::timing::RequestTiming) {
        use std::cmp::max;

        let dns_ms = timing.dns_lookup.as_millis();
        let tcp_ms = timing.tcp_connect.as_millis();
        let ttfb_ms = timing.ttfb.as_millis();
        let transfer_ms = timing.transfer.as_millis();

        let total_ms = dns_ms + tcp_ms + ttfb_ms + transfer_ms;
        if total_ms == 0 {
            return;
        }

        println!("   Network Diagnostics (Total: {}ms):", total_ms);

        let render_bar = |name: &str, ms: u128, color: &str| {
            let percentage = (ms as f64 / total_ms as f64) * 100.0;
            // 设定进度条最大字符长度为 30
            let bar_len = ((ms as f64 / total_ms as f64) * 30.0).round() as usize;
            let bar_len = max(bar_len, if ms > 0 { 1 } else { 0 });

            let bar = "■".repeat(bar_len);
            let empty = " ".repeat(30 - bar_len);

            let colored_bar = match color {
                "blue" => bar.blue(),
                "yellow" => bar.yellow(),
                "green" => bar.green(),
                "cyan" => bar.cyan(),
                _ => bar.white(),
            };

            println!(
                "     {:12} : {}{} ({:3}ms, {:.1}%)",
                name.dimmed(),
                colored_bar,
                empty.dimmed(),
                ms,
                percentage
            );
        };

        render_bar("DNS Lookup", dns_ms, "blue");
        render_bar("TCP Connect", tcp_ms, "yellow");
        render_bar("Server Wait", ttfb_ms, "green");
        render_bar("Transfer", transfer_ms, "cyan");
        println!();
    }

    /// 打印测试开始
    pub fn print_header(&self, file_path: &str, total: usize) {
        println!(
            "\nRunning {} requests from {}...\n",
            total,
            file_path.bold()
        );
    }

    /// 打印测试摘要
    pub fn print_summary(&self, summary: &TestSummary) {
        println!("\n{}", "━".repeat(50));
        println!("{}", "Summary".bold());
        println!("{}", "━".repeat(50));

        if summary.skipped > 0 {
            println!(
                "  {}: {} passed, {} failed, {} skipped, {} total",
                "Tests".bold(),
                summary.passed.to_string().green(),
                summary.failed.to_string().red(),
                summary.skipped.to_string().dimmed(),
                summary.total
            );
        } else if summary.failed == 0 {
            println!(
                "  {}: {} passed, {} total",
                "Tests".bold(),
                summary.passed.to_string().green(),
                summary.total
            );
        } else {
            println!(
                "  {}: {} passed, {} failed, {} total",
                "Tests".bold(),
                summary.passed.to_string().green(),
                summary.failed.to_string().red(),
                summary.total
            );
        }

        // 显示断言统计
        if summary.total_assertions > 0 {
            if summary.failed_assertions == 0 {
                println!(
                    "  {}: {} passed, {} total",
                    "Assertions".bold(),
                    summary.passed_assertions.to_string().green(),
                    summary.total_assertions
                );
            } else {
                println!(
                    "  {}: {} passed, {} failed, {} total",
                    "Assertions".bold(),
                    summary.passed_assertions.to_string().green(),
                    summary.failed_assertions.to_string().red(),
                    summary.total_assertions
                );
            }
        }

        println!(
            "  {}: {:.3}s",
            "Duration".bold(),
            summary.total_duration.as_secs_f64()
        );
        println!();
    }

    /// 将内存测试结果转换为强类型报告（对失败请求的响应片段与 URL 脱敏并 512B 截断）
    pub fn build_json_reports(
        results: &[(std::path::PathBuf, Vec<TestResult>)],
    ) -> Vec<JsonFileReport> {
        results
            .iter()
            .map(|(path, file_results)| JsonFileReport {
                file_path: path.to_string_lossy().to_string(),
                results: file_results
                    .iter()
                    .map(|r| {
                        let assertions = r
                            .assertions
                            .iter()
                            .map(|a| {
                                // Gap Review P1-2：expected/actual/message 可能携带响应体敏感值
                                // （如 body.access_token 的实际取值），与 snippet 同级脱敏
                                JsonAssertionReport {
                                    raw: a.raw.clone(),
                                    passed: a.passed,
                                    message: crate::ai::redactor::redact_assertion_value(
                                        &a.raw,
                                        a.message.as_deref(),
                                    ),
                                    expected: crate::ai::redactor::redact_assertion_value(
                                        &a.raw,
                                        (!a.expected.is_empty()).then_some(a.expected.as_str()),
                                    ),
                                    actual: crate::ai::redactor::redact_assertion_value(
                                        &a.raw,
                                        a.actual.as_deref(),
                                    ),
                                    stream_event_index: a.stream_event_index,
                                }
                            })
                            .collect();

                        let response_body_snippet = if !r.success {
                            r.response
                                .as_ref()
                                .map(|resp| {
                                    crate::ai::redactor::redact_and_truncate_body(
                                        &resp.body,
                                        crate::ai::redactor::DEFAULT_SNIPPET_MAX_BYTES,
                                    )
                                })
                                .filter(|s| !s.is_empty())
                        } else {
                            None
                        };

                        JsonTestReport {
                            request_number: r.request_number,
                            name: r.name.clone(),
                            method: r.method.clone(),
                            url: crate::ai::redactor::redact_url(&r.url),
                            status: r.status,
                            duration_ms: r.duration.as_millis() as u64,
                            success: r.success,
                            // Gap Review P1-1：reqwest 错误文本自带完整原始 URL，必须掩码
                            error: r
                                .error
                                .as_deref()
                                .map(crate::ai::redactor::redact_free_text),
                            skipped: r.skipped,
                            assertions,
                            // 报告级红线：warnings 值源（未定义变量名/位置错误上下文）统一脱敏
                            warnings: r
                                .warnings
                                .iter()
                                .map(|w| crate::ai::redactor::redact_free_text(w))
                                .collect(),
                            // diagnose 的 URL 来自 --debug 实测原始地址，出站前与顶层 url 同口径脱敏
                            diagnose_report: r.diagnose_report.clone().map(|mut diag| {
                                diag.url = crate::ai::redactor::redact_url(&diag.url);
                                diag
                            }),
                            response_body_snippet,
                        }
                    })
                    .collect(),
            })
            .collect()
    }

    /// 导出为 JSON 报告
    pub fn report_json(
        results: &[(std::path::PathBuf, Vec<TestResult>)],
        writer: &mut impl std::io::Write,
    ) -> crate::Result<()> {
        let json_list = Self::build_json_reports(results);
        let json_str = serde_json::to_string_pretty(&json_list)?;
        writer.write_all(json_str.as_bytes())?;
        Ok(())
    }

    /// 打印批量测试汇总
    pub fn print_batch_summary(
        results: &[(std::path::PathBuf, Vec<TestResult>)],
        duration: std::time::Duration,
    ) {
        let total_files = results.len();
        let mut passed_files = 0;
        let mut failed_files = 0;

        let mut total_tests = 0;
        let mut passed_tests = 0;
        let mut failed_tests = 0;

        for (_, file_results) in results {
            let file_failed = file_results.iter().any(|r| !r.success);
            if file_failed {
                failed_files += 1;
            } else {
                passed_files += 1;
            }

            for r in file_results {
                total_tests += 1;
                if r.success {
                    passed_tests += 1;
                } else {
                    failed_tests += 1;
                }
            }
        }

        println!("\n{}", "━".repeat(50).bold());
        println!("{}", "Batch Test Summary".bold());
        println!("{}", "━".repeat(50).bold());

        println!(
            "  Files Ran: {} total ({} passed, {} failed)",
            total_files,
            passed_files.to_string().green(),
            failed_files.to_string().red()
        );
        println!(
            "  Tests:     {} passed, {} failed, {} total",
            passed_tests.to_string().green(),
            failed_tests.to_string().red(),
            total_tests
        );
        println!("  Duration:  {:.3}s", duration.as_secs_f64());
        println!();
    }
}

impl Default for TestReporter {
    fn default() -> Self {
        Self::new(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::TestResult;
    use std::time::Duration;

    #[test]
    fn test_json_report_includes_warnings() {
        // 回归：JSON 报告此前手写字段清单漏掉 warnings——CI 用户（jq 解析）
        // 拿不到任何捕获失败/未定义变量警告
        let mut result = TestResult::error(
            1,
            Some("t".to_string()),
            "GET".to_string(),
            "http://example.com".to_string(),
            "boom".to_string(),
            Duration::from_millis(1),
        );
        result
            .warnings
            .push("变量 {{x}} 未定义，已按字面量发送".to_string());

        let results = vec![(std::path::PathBuf::from("a.http"), vec![result])];
        let mut out = Vec::new();
        TestReporter::report_json(&results, &mut out).unwrap();

        let text = String::from_utf8(out).unwrap();
        assert!(
            text.contains("\"warnings\""),
            "JSON 报告应包含 warnings 字段"
        );
        assert!(text.contains("未定义"), "警告内容应完整输出: {text}");
    }

    #[test]
    fn test_json_report_round_trip_with_assertions_and_redacted_snippet() {
        use crate::assertion::AssertionResult;
        use crate::http::Response;
        use reqwest::header::HeaderMap;

        let resp = Response::new(
            401,
            HeaderMap::new(),
            r#"{"error":"invalid_token","access_token":"secret-xyz"}"#.to_string(),
            Duration::from_millis(12),
            Duration::from_millis(5),
            Duration::from_millis(7),
        )
        .unwrap();

        let mut result = TestResult::success(
            1,
            Some("AuthCheck".to_string()),
            "POST".to_string(),
            "https://api.example.com/login?api_key=12345".to_string(),
            resp,
        );
        result.success = false;
        // 注意 AssertionResult::failure 参数顺序：(raw, actual, expected, message)
        result.assertions.push(AssertionResult::failure(
            "status == 200".to_string(),
            "401".to_string(),
            "200".to_string(),
            "Status code mismatch".to_string(),
        ));

        let batch = vec![(std::path::PathBuf::from("auth.http"), vec![result])];
        let mut buf = Vec::new();
        TestReporter::report_json(&batch, &mut buf).unwrap();

        let parsed: Vec<JsonFileReport> = serde_json::from_slice(&buf).unwrap();
        assert_eq!(parsed.len(), 1);
        let req = &parsed[0].results[0];
        assert!(
            req.url.contains("api_key=%5BREDACTED%5D") || req.url.contains("api_key=[REDACTED]")
        );
        assert_eq!(req.assertions[0].expected.as_deref(), Some("200"));
        assert_eq!(req.assertions[0].actual.as_deref(), Some("401"));
        assert_eq!(
            req.assertions[0].message.as_deref(),
            Some("Status code mismatch")
        );
        let snippet = req.response_body_snippet.as_deref().unwrap();
        assert!(snippet.contains(r#""access_token":"[REDACTED]""#));
        assert!(!snippet.contains("secret-xyz"));
    }
}
