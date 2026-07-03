mod cli;

use clap::Parser;
use cli::{Cli, Commands, HistoryCommands};
use rupost::Result;
use rupost::generator::http::HttpGenerator;
use rupost::history::selector::{self, SelectionStrategy};
use rupost::history::storage::get_storage;
use std::fs;

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化 Rustls 默认 CryptoProvider 解决 wss:// 协议协商崩溃问题
    let _ = rustls::crypto::ring::default_provider().install_default();

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
            save_snapshot,
        }) => {
            let options = rupost::runner::TestSuiteOptions {
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
                debug: cli.debug,
                debug_on_failure: cli.debug_on_failure,
                default_scheme: cli.default_scheme,
                save_snapshot,
            };
            let has_failure = rupost::runner::run_test_suite(options).await?;
            if has_failure {
                std::process::exit(1);
            }
        }
        Some(Commands::History { command }) => match command {
            HistoryCommands::List { limit, reverse } => {
                rupost::history::printer::list_history(limit, reverse)?;
            }
            HistoryCommands::Export { last: _, output: _ } => {
                println!("History export is not implemented in MVP stage.");
            }
        },
        Some(Commands::Diagnose { url, report }) => match rupost::http::diagnose_url(&url).await {
            Ok(report_data) => {
                if report == "json" {
                    println!("{}", serde_json::to_string_pretty(&report_data).unwrap());
                } else {
                    rupost::http::print_diagnose_report(&report_data);
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
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
        Some(Commands::Mock { file, port }) => {
            rupost::mock::run_server_from_file(&file, port).await?;
        }
        Some(Commands::Init {
            r#type,
            output,
            force,
            list,
        }) => {
            rupost::template::run_template(&r#type, output.as_deref(), force, list)?;
        }
        Some(Commands::Replay {
            file,
            target,
            verbose,
        }) => {
            let replayer = rupost::runner::ReplayExecutor::new(target, verbose);
            replayer.replay_file(&file).await?;
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
                    cli.default_scheme,
                    cli.debug,
                    cli.debug_on_failure,
                )
                .await?;
            }
        }
    }
    Ok(())
}
