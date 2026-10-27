mod cli;

use clap::Parser;
use cli::{AiCommands, Cli, Commands, HistoryCommands};
use colored::Colorize;
use rupost::{Result, RupostError};
use rupost_ai::{AssertionTranslator, OpenAiClient};

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
                templatize_host: args.templatize_host,
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
        Some(Commands::Ai { command }) => match command {
            AiCommands::Assert { text, raw } => {
                if let Err(e) = run_ai_assert(&text, raw).await {
                    eprintln!("{} {}", "[ERROR]".red().bold(), e.to_user_friendly_string());
                    std::process::exit(1);
                }
            }
        },
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

/// `rupost ai assert`: 将自然语言期望翻译为标准 @assert 断言行（创作期辅助，密钥仅从环境变量读取）
async fn run_ai_assert(text: &str, raw: bool) -> Result<()> {
    let api_key = std::env::var("RUPOST_AI_API_KEY")
        .ok()
        .filter(|key| !key.is_empty())
        .or_else(|| {
            std::env::var("OPENAI_API_KEY")
                .ok()
                .filter(|key| !key.is_empty())
        })
        .ok_or_else(|| {
            RupostError::Other(
                "未配置 AI 密钥：请先 `export RUPOST_AI_API_KEY=...`（或 `OPENAI_API_KEY`）；\
                 可选 `RUPOST_AI_BASE_URL` 指定 OpenAI 兼容端点、`RUPOST_AI_MODEL` 指定模型"
                    .to_string(),
            )
        })?;
    let base_url = std::env::var("RUPOST_AI_BASE_URL").ok();
    let model = std::env::var("RUPOST_AI_MODEL").ok();
    let client = OpenAiClient::new(api_key, base_url, model);
    let translator = AssertionTranslator::new(std::sync::Arc::new(client));
    let assertions = translator
        .translate(text)
        .await
        .map_err(|e| RupostError::Other(truncate_message(&e.to_string(), 500)))?;

    if raw {
        for assertion in &assertions {
            println!("# @assert {assertion}");
        }
    } else {
        println!(
            "{} 已生成 {} 条标准断言（粘贴到请求块末尾即可生效）:",
            "[✓]".green().bold(),
            assertions.len()
        );
        for assertion in &assertions {
            println!("# @assert {assertion}");
        }
    }
    Ok(())
}

/// 错误信息截断，防止 AI 返回的超长响应体刷屏
fn truncate_message(msg: &str, max_chars: usize) -> String {
    if msg.chars().count() <= max_chars {
        msg.to_string()
    } else {
        let truncated: String = msg.chars().take(max_chars).collect();
        format!("{truncated}...[truncated]")
    }
}
