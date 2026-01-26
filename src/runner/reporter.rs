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

        // 3. 如果有错误消息（转换或网络错误），显示错误
        if let Some(error) = &result.error {
            println!("   {}: {}", "Error".red().bold(), error);
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

        if summary.failed == 0 {
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
