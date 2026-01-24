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
        Some(Commands::Test {
            path,
            env,
            var,
            verbose,
        }) => {
            run_test(&path, env.as_deref(), &var, verbose).await?;
        }
        Some(Commands::History { command }) => match command {
            cli::HistoryCommands::List { limit } => {
                rupost::history::printer::list_history(limit)?;
            }
        },
        Some(Commands::Generate(args)) => {
            use rupost::generator::http::HttpGenerator;
            use rupost::history::storage::get_storage;
            use std::fs;

            let storage = get_storage();
            let entries = storage.tail(args.last)?;

            if entries.is_empty() {
                eprintln!("No history found to generate.");
                return Ok(());
            }

            let content = HttpGenerator::generate(&entries)?;
            fs::write(&args.output_file, content)?;
            println!(
                "Generated test file: {} ({} requests)",
                args.output_file,
                entries.len()
            );
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

async fn run_test(
    file_path: &str,
    env_name: Option<&str>,
    var_overrides: &[String],
    verbose: bool,
) -> Result<()> {
    use rupost::parser::{HttpFileParser, MarkdownFileParser};
    use rupost::runner::{TestExecutor, TestReporter, TestSummary};
    use rupost::variable::{ConfigLoader, VariableContext};
    use std::path::Path;

    // 1. 加载配置并构建变量上下文
    let mut var_context = if env_name.is_some() || !var_overrides.is_empty() {
        let config = ConfigLoader::find_and_load().unwrap_or_default();

        // 解析 CLI 变量覆盖
        let cli_vars: Vec<(String, String)> = var_overrides
            .iter()
            .filter_map(|s| ConfigLoader::parse_cli_var(s))
            .collect();

        ConfigLoader::build_context(&config, env_name, &cli_vars)
    } else {
        VariableContext::new()
    };

    // 2. 根据文件扩展名选择解析器
    let path = Path::new(file_path);
    let parsed_file = if path.extension().and_then(|s| s.to_str()) == Some("md") {
        MarkdownFileParser::parse_file(path)?
    } else {
        HttpFileParser::parse_file(path)?
    };

    let total = parsed_file.requests.len();

    // 4. 创建报告器并打印开始信息
    let reporter = TestReporter::new(verbose);
    reporter.print_header(file_path, total);

    // 5. 执行所有请求
    let executor = TestExecutor::new();
    let results = executor.execute_all(parsed_file, &mut var_context).await?;

    // 6. 打印每个结果
    for result in &results {
        reporter.print_result(result);
    }

    // 7. 打印摘要
    let summary = TestSummary::from_results(&results);
    reporter.print_summary(&summary);

    // 8. 设置退出码
    if summary.failed > 0 {
        std::process::exit(1);
    }

    Ok(())
}
