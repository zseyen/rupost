mod cli;

use clap::Parser;
use cli::{Cli, Commands, HistoryCommands};
use rupost::Result;

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
            HistoryCommands::Show { target } => {
                rupost::history::printer::show_history(&target)?;
            }
            HistoryCommands::Prune { days, max_size } => {
                rupost::history::printer::prune_history(days, max_size)?;
            }
            HistoryCommands::Clear { yes, all } => {
                rupost::history::printer::clear_history(yes, all)?;
            }
            HistoryCommands::Export { last: _, output: _ } => {
                println!("History export is not implemented in MVP stage.");
            }
        },
        Some(Commands::Diagnose { url, report }) => {
            rupost::http::run_diagnose(&url, &report).await?;
        }
        Some(Commands::Generate(args)) => {
            let opts = rupost::generator::GenerateOptions {
                output_file: args.output_file,
                last: args.last,
                interactive: args.interactive,
            };
            rupost::generator::run_generate(opts)?;
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
        Some(Commands::Export(args)) => {
            let opts = args.into_run_options(cli.env, cli.cookie_file, cli.default_scheme);
            rupost::exporter::run_export(opts)?;
        }
        Some(Commands::Tui { file }) => {
            rupost::tui::run(file).await?;
        }
        None => {
            if cli.args.is_empty() {
                tracing::error!("No command provided");
                std::process::exit(1);
            } else {
                cli::run(cli::RunParams::from_cli(&cli)).await?;
            }
        }
    }
    Ok(())
}
