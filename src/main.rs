mod cli;

use clap::Parser;
use cli::{Cli, Commands};
use rupost::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志系统
    rupost::logger::init_logger();

    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Test { path, verbose }) => {
            run_test(&path, verbose).await?;
        }
        None => {
            if cli.args.is_empty() {
                eprintln!("No command provided");
                std::process::exit(1);
            } else {
                cli::run(cli.args).await?;
            }
        }
    }
    Ok(())
}

async fn run_test(file_path: &str, verbose: bool) -> Result<()> {
    use rupost::parser::{HttpFileParser, MarkdownFileParser};
    use rupost::runner::{TestExecutor, TestReporter, TestSummary};
    use std::path::Path;

    // 1. 根据文件扩展名选择解析器
    let path = Path::new(file_path);
    let parsed_file = if path.extension().and_then(|s| s.to_str()) == Some("md") {
        MarkdownFileParser::parse_file(path)?
    } else {
        HttpFileParser::parse_file(path)?
    };

    let total = parsed_file.requests.len();

    // 2. 创建报告器并打印开始信息
    let reporter = TestReporter::new(verbose);
    reporter.print_header(file_path, total);

    // 3. 执行所有请求
    let executor = TestExecutor::new();
    let results = executor.execute_all(parsed_file).await?;

    // 4. 打印每个结果
    for result in &results {
        reporter.print_result(result);
    }

    // 5. 打印摘要
    let summary = TestSummary::from_results(&results);
    reporter.print_summary(&summary);

    // 6. 设置退出码
    if summary.failed > 0 {
        std::process::exit(1);
    }

    Ok(())
}
