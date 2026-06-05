mod cli;

use clap::Parser;
use cli::{Cli, Commands, HistoryCommands};
use rupost::Result;
use rupost::generator::http::HttpGenerator;
use rupost::history::selector::{self, SelectionStrategy};
use rupost::history::storage::get_storage;
use rupost::middleware::resolve_cookie_path;
use rupost::runner::{
    BatchExecutor, BatchMode, BatchRunRequest, DependencyResolver, DirectoryScanner, TestExecutor,
    TestReporter, WorkflowGraph,
};
use rupost::variable::{ConfigLoader, VariableContext};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

struct RunTestOptions<'a> {
    paths: Vec<String>,
    mode: &'a str,
    concurrency: usize,
    report: &'a str,
    fail_fast: bool,
    env_name: Option<&'a str>,
    var_overrides: &'a [String],
    env_file: Option<&'a str>,
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
            paths,
            mode,
            concurrency,
            report,
            fail_fast,
            env,
            var,
            env_file,
            verbose,
            no_cookies,
            cookie_file,
        }) => {
            let options = RunTestOptions {
                paths,
                mode: &mode,
                concurrency,
                report: &report,
                fail_fast,
                env_name: env.as_deref(),
                var_overrides: &var,
                env_file: env_file.as_deref(),
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
    // 1. 递归扫描文件
    let scanned_files = DirectoryScanner::scan(&options.paths)?;
    if scanned_files.is_empty() {
        println!("No test files found.");
        return Ok(());
    }

    // 2. 递归自动补全加载所有依赖（带安全沙箱限制）
    let sandbox_root = std::env::current_dir()?;
    let files_map = DependencyResolver::resolve_and_parse(&scanned_files, &sandbox_root)?;

    // 3. 拓扑排序解析依赖
    let parse_pairs: Vec<_> = files_map
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let graph = WorkflowGraph::new(&parse_pairs);
    let execution_order = graph.resolve_execution_order()?;

    // 4. 加载配置并构建变量上下文
    let config = ConfigLoader::find_and_load().unwrap_or_default();

    // 解析 CLI 变量覆盖
    let cli_vars: Vec<(String, String)> = options
        .var_overrides
        .iter()
        .filter_map(|s| ConfigLoader::parse_cli_var(s))
        .collect();

    // 如果指定了 env_file 则使用它，否则默认自动寻找加载并合并本地 .env
    let env_file_to_load = options.env_file.or(Some(".env"));

    let mut var_context =
        ConfigLoader::build_context(&config, options.env_name, &cli_vars, env_file_to_load);

    // 5. 构建 TestExecutor
    let mut executor = if options.no_cookies {
        TestExecutor::new()
    } else if let Some(cookie_path) = &options.cookie_file {
        let resolved = resolve_cookie_path(PathBuf::from(cookie_path), options.env_name);
        TestExecutor::with_cookies(resolved)?
    } else {
        TestExecutor::with_ephemeral_cookies()
    };

    executor = executor
        .with_debug(options.debug)
        .with_debug_on_failure(options.debug_on_failure);

    let batch_executor = BatchExecutor::new(executor);

    // 6. 执行批处理
    let start_time = Instant::now();
    // 提取依赖映射关系，传入 execute_batch 供并行调度使用
    let dependencies: HashMap<PathBuf, Vec<PathBuf>> = graph
        .nodes
        .iter()
        .map(|(k, v)| (k.clone(), v.depends_on.clone()))
        .collect();

    let batch_mode = if options.mode == "parallel" {
        BatchMode::Parallel
    } else {
        BatchMode::Serial
    };

    let batch_request = BatchRunRequest {
        execution_order,
        dependencies,
        files_map,
        context: &mut var_context,
        mode: batch_mode,
        concurrency: options.concurrency,
        fail_fast: options.fail_fast,
    };

    let batch_results = batch_executor.execute_batch(batch_request).await?;
    let duration = start_time.elapsed();

    // 7. 渲染与汇报结果
    let has_failure = batch_results
        .iter()
        .any(|(_, res_list)| res_list.iter().any(|r| !r.success));

    if options.report == "json" {
        TestReporter::report_json(&batch_results, &mut std::io::stdout())?;
    } else {
        let reporter = TestReporter::new(options.verbose);
        for (path, file_results) in &batch_results {
            let path_str = path.to_string_lossy().to_string();
            reporter.print_header(&path_str, file_results.len());
            for r in file_results {
                reporter.print_result(r);
            }
        }
        TestReporter::print_batch_summary(&batch_results, duration);
    }

    // 8. 设置退出码
    if has_failure {
        std::process::exit(1);
    }

    Ok(())
}
