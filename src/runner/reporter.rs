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
        if (self.verbose || !result.success) && result.response.is_some() {
            let response = result.response.as_ref().unwrap();
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
}

impl Default for TestReporter {
    fn default() -> Self {
        Self::new(false)
    }
}
