//! 命令派发总入口（自 main.rs 下沉后于解耦提案 v2 Phase 4 归位本模块；
//! main.rs 保持无业务逻辑）。

use super::Cli;
use super::commands::{AiCommands, Commands, HistoryCommands, ImportCommands};
use super::runner::{self, RunParams};
use rupost::Result;

/// 命令派发总入口
pub async fn dispatch(cli: Cli) -> Result<()> {
    use colored::Colorize;

    match cli.command {
        Some(Commands::Test(args)) => {
            let crate::cli::TestArgs {
                paths,
                mode,
                concurrency,
                report,
                report_file,
                explain,
                triage,
                fail_fast,
                env,
                var,
                env_file,
                verbose,
                no_cookies,
                cookie_file,
                save_snapshot,
                mask_secrets,
                only,
                on_failure,
            } = args;

            // --explain 兼容环境变量开关（flag 未传时 RUPOST_AI_EXPLAIN=1/true/yes/on 生效）
            let explain_enabled = explain
                || std::env::var("RUPOST_AI_EXPLAIN").is_ok_and(|v| {
                    matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
                });
            let env_label = env.clone();
            let options = rupost::runner::TestSuiteOptions {
                paths,
                mode,
                concurrency,
                report: report.clone(),
                report_file: report_file.clone(),
                explain: explain_enabled,
                triage,
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
                mask_snapshot: mask_secrets,
                only,
                on_failure,
            };
            let outcome = rupost::runner::run_test_suite(options).await?;

            // 报告文件输出（表现层职责，UseCase 不再隐式落盘）：
            // 覆盖提示走 stderr；"已生成"提示仅在 HTML 格式或 verbose 时走 stdout。
            // 空扫描（无 .http/.md）恢复旧语义：不产生报告文件（审查 P1）
            if !outcome.batch_results.is_empty()
                && let Some(written) = rupost::report::write_report_files(
                    &outcome.batch_results,
                    &report,
                    report_file.as_deref(),
                    env_label.as_deref(),
                )?
            {
                if written.overwritten {
                    eprintln!("[!] 报告文件已存在，将覆盖: {}", written.path.display());
                }
                if matches!(written.format, rupost::report::ReportFileFormat::Html)
                    && (verbose || report.eq_ignore_ascii_case("html"))
                {
                    println!("  报告已生成至: {}", written.path.display());
                }
            }

            if outcome.has_failure {
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
            HistoryCommands::Export {
                last,
                output,
                mask_secrets,
            } => {
                rupost::history::export_history_snapshot(last, &output, mask_secrets)?;
            }
        },
        Some(Commands::Diagnose(args)) => {
            rupost::http::run_diagnose(&args.url, &args.report).await?;
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
        Some(Commands::Mock(args)) => {
            rupost::mock::run_server_from_file(&args.file, args.port).await?;
        }
        Some(Commands::Init(args)) => {
            rupost::template::run_template(
                &args.r#type,
                args.output.as_deref(),
                args.force,
                args.list,
            )?;
        }
        Some(Commands::Replay(args)) => {
            let mut replayer = rupost::runner::ReplayExecutor::new(args.target, args.verbose);
            if let Some(keys) = args.ignore_fields {
                let parsed: Vec<String> = keys
                    .split(',')
                    .map(|k| k.trim().to_string())
                    .filter(|k| !k.is_empty())
                    .collect();
                replayer = replayer.with_ignore_fields(parsed);
            }
            if args.status_only {
                replayer = replayer.status_only();
            }
            let report = replayer.replay_file(&args.file).await?;
            // P1 AI 接线：失败条目就地归因（规则版基线 + AI 增强；B1 降级语义同 t --explain）
            if args.explain && report.failed > 0 {
                let mut session = rupost::ai::explain::ExplainSession::new();
                let client = match rupost::ai::config::resolve().await {
                    Ok(resolved) => Some(resolved.client),
                    Err(msg) => {
                        eprintln!("    {} {}", "⚠".yellow(), msg);
                        None
                    }
                };
                for entry in report.entries.iter().filter(|e| !e.matched) {
                    let ctx = rupost::ai::rules::context_from_replay_entry(entry);
                    session
                        .explain_context_with_client(ctx, client.as_deref())
                        .await;
                }
            }
            if report.failed > 0 {
                std::process::exit(1);
            }
        }
        Some(Commands::Export(args)) => {
            let opts = args.into_run_options(cli.env, cli.cookie_file, cli.default_scheme);
            rupost::exporter::run_export(opts)?;
        }
        Some(Commands::Ai { command }) => match command {
            AiCommands::Assert {
                text,
                last,
                response,
                no_mask,
                raw,
            } => {
                let params = rupost::ai::AiAssertParams {
                    raw,
                    last,
                    response,
                    no_mask,
                };
                if let Err(e) = rupost::ai::run_ai_assert(&params, &text).await {
                    eprintln!("{} {}", "[ERROR]".red().bold(), e.to_user_friendly_string());
                    std::process::exit(1);
                }
            }
            AiCommands::Triage {
                report_file,
                json,
                format,
            } => {
                rupost::ai::triage::run_ai_triage_from_file(&report_file, json, &format).await?;
            }
            AiCommands::Resolve { file, dry_run } => {
                rupost::ai::resolve::run_resolve(&file, dry_run).await?;
            }
            AiCommands::Bootstrap {
                from,
                last,
                filter,
                output,
                no_ai,
                templatize_host,
            } => {
                let params = rupost::ai::bootstrap::BootstrapParams {
                    from,
                    last,
                    filter,
                    output,
                    no_ai,
                    templatize_host,
                };
                rupost::ai::bootstrap::run_bootstrap(&params).await?;
            }
        },
        Some(Commands::Import { command }) => match command {
            ImportCommands::Curl(args) => rupost::importer::run_curl_import(&args.into_request())?,
        },
        Some(Commands::Tui(args)) => {
            rupost::tui::run(
                args.file,
                rupost::tui::TuiOptions {
                    env: args.env,
                    vars: args.var,
                    env_file: args.env_file,
                    cookie_file: args.cookie_file,
                },
            )
            .await?;
        }
        Some(Commands::Web(args)) => {
            let store: std::sync::Arc<dyn rupost::web::DocumentStore> =
                std::sync::Arc::new(rupost::web::LocalFsDocumentStore::new(&args.dir));

            let assets: std::sync::Arc<dyn rupost::web::AssetProvider> = match args.static_dir {
                Some(d) => std::sync::Arc::new(rupost::web::LocalDirAssetProvider::new(d)),
                None => std::sync::Arc::new(rupost::web::EmbeddedAssetProvider),
            };

            let config = rupost::web::WebServerConfig {
                store,
                assets,
                readonly: args.readonly,
                working_dir: std::path::PathBuf::from(&args.dir),
                default_env: cli.env.clone(),
                allowed_hosts: args.allowed_hosts,
                dev: args.dev,
            };

            rupost::web::run_web_server(config, &args.host, args.port).await?;
        }
        None => {
            if cli.args.is_empty() {
                tracing::error!("No command provided");
                std::process::exit(1);
            } else {
                runner::run(RunParams::from_cli(&cli)).await?;
            }
        }
    }
    Ok(())
}
