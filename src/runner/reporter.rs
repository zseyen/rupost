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

        // 显示网络时序诊断
        if let Some(ref timing) = result.timing {
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

    /// 导出为 JSON 报告
    pub fn report_json(
        results: &[(std::path::PathBuf, Vec<TestResult>)],
        writer: &mut impl std::io::Write,
    ) -> crate::Result<()> {
        let mut json_list = Vec::new();
        for (path, file_results) in results {
            let mut req_results = Vec::new();
            for r in file_results {
                let assertions_json: Vec<serde_json::Value> = r
                    .assertions
                    .iter()
                    .map(|a| {
                        serde_json::json!({
                            "raw": a.raw,
                            "passed": a.passed,
                            "message": a.message,
                        })
                    })
                    .collect();

                req_results.push(serde_json::json!({
                    "request_number": r.request_number,
                    "name": r.name,
                    "method": r.method,
                    "url": r.url,
                    "status": r.status,
                    "duration_ms": r.duration.as_millis(),
                    "success": r.success,
                    "error": r.error,
                    "skipped": r.skipped,
                    "assertions": assertions_json,
                }));
            }
            json_list.push(serde_json::json!({
                "file_path": path.to_string_lossy(),
                "results": req_results,
            }));
        }

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
