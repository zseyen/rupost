mod cli;

use clap::Parser;
use cli::{Cli, Commands, HistoryCommands};
use rupost::generator::http::HttpGenerator;
use rupost::history::selector::{self, SelectionStrategy};
use rupost::history::storage::get_storage;
use rupost::middleware::resolve_cookie_path;
use rupost::parser::{HttpFileParser, MarkdownFileParser};
use rupost::runner::{TestExecutor, TestReporter, TestSummary};
use rupost::variable::{ConfigLoader, VariableContext};
use rupost::Result;
use std::fs;
use std::path::{Path, PathBuf};

struct RunTestOptions<'a> {
    file_path: &'a str,
    env_name: Option<&'a str>,
    var_overrides: &'a [String],
    verbose: bool,
    no_cookies: bool,
    cookie_file: Option<String>,
    debug: bool,
    debug_on_failure: bool,
}

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
            no_cookies,
            cookie_file,
        }) => {
            let options = RunTestOptions {
                file_path: &path,
                env_name: env.as_deref(),
                var_overrides: &var,
                verbose,
                no_cookies,
                cookie_file,
                debug: cli.debug,
                debug_on_failure: cli.debug_on_failure,
            };
            run_test(options).await?;
        }
        Some(Commands::History { command }) => match command {
            HistoryCommands::List { limit, reverse } => {
                rupost::history::printer::list_history(limit, reverse)?;
            }
        },
        Some(Commands::Generate(args)) => {
            let storage = get_storage();

            // Determine strategy
            let strategy = if args.interactive {
                SelectionStrategy::Interactive
            } else {
                SelectionStrategy::Last(args.last)
            };

            // Execute selection
            let entries = selector::select_entries(storage, strategy)?;

            if entries.is_empty() {
                tracing::warn!("No history found or selected to generate.");
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
                tracing::error!("No command provided");
                std::process::exit(1);
            } else {
                cli::run(
                    cli.args,
                    cli.no_cookies,
                    cli.cookie_file,
                    cli.debug,
                    cli.debug_on_failure,
                )
                .await?;
            }
        }
    }
    Ok(())
}

async fn run_test(options: RunTestOptions<'_>) -> Result<()> {
    // 1. 加载配置并构建变量上下文
    let mut var_context = if options.env_name.is_some() || !options.var_overrides.is_empty() {
        let config = ConfigLoader::find_and_load().unwrap_or_default();

        // 解析 CLI 变量覆盖
        let cli_vars: Vec<(String, String)> = options
            .var_overrides
            .iter()
            .filter_map(|s| ConfigLoader::parse_cli_var(s))
            .collect();

        ConfigLoader::build_context(&config, options.env_name, &cli_vars)
    } else {
        VariableContext::new()
    };

    // 2. 根据文件扩展名选择解析器
    let path = Path::new(options.file_path);
    let parsed_file = if path.extension().and_then(|s| s.to_str()) == Some("md") {
        MarkdownFileParser::parse_file(path)?
    } else {
        HttpFileParser::parse_file(path)?
    };

    let total = parsed_file.requests.len();

    // 4. 创建报告器并打印开始信息
    let reporter = TestReporter::new(options.verbose);
    reporter.print_header(options.file_path, total);

    // 5. 执行所有请求
    let mut executor = if options.no_cookies {
        TestExecutor::new()
    } else if let Some(cookie_path) = options.cookie_file {
        let resolved = resolve_cookie_path(PathBuf::from(cookie_path), options.env_name);
        TestExecutor::with_cookies(resolved)?
    } else {
        TestExecutor::with_ephemeral_cookies()
    };

    executor = executor
        .with_debug(options.debug)
        .with_debug_on_failure(options.debug_on_failure);
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
