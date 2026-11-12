//! 命令派发总入口（自 main.rs 下沉后于解耦提案 v2 Phase 4 归位本模块；
//! main.rs 保持无业务逻辑）。

use super::Cli;
use super::commands::{AiCommands, Commands, HistoryCommands, ImportCommands};
use super::runner::{self, RunParams};
use rupost::Result;

/// 命令派发总入口
pub async fn dispatch(cli: Cli) -> Result<()> {
    use colored::Colorize;

    // --no-history 必须在任何命令执行前关停记录器（进程级不可逆全局开关，
    // CI/共享环境防凭据明文落盘；统一覆盖 Test、Tui、Web、Replay 与内联模式；提示走 stderr）
    if cli.no_history {
        rupost::history::recorder::disable_history_recording();
        eprintln!("[i] 历史记录已停用 (--no-history)：本次运行不写入 history.jsonl");
    }

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
                max_snapshot_body_bytes,
                timing,
                only,
                on_failure,
                allow_missing_plugins,
                strict_plugins,
                plugin_timeout,
                plugin_env,
                allow_project_plugins,
            } = args;

            // --explain 兼容环境变量开关（flag 未传时 RUPOST_AI_EXPLAIN=1/true/yes/on 生效）
            let explain_enabled =
                explain || rupost::security::fs::env_flag_enabled("RUPOST_AI_EXPLAIN");
            // --timing 仅支持 off（02 §2:135 契约：none = 未采集）；未知模式按语法错误 exit 2
            let timing_off = match timing.as_deref() {
                None => false,
                Some("off") => true,
                Some(other) => {
                    return Err(rupost::RupostError::ParseError(format!(
                        "无效的 --timing 模式 '{other}'（当前仅支持: off）"
                    )));
                }
            };
            let options = rupost::runner::TestSuiteOptions {
                paths,
                mode,
                concurrency,
                report,
                report_file,
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
                max_snapshot_body_bytes,
                timing_off,
                only,
                on_failure,
                allow_missing_plugins,
                strict_plugins,
                plugin_timeout_secs: plugin_timeout,
                plugin_env,
                allow_project_plugins,
            };
            let outcome = rupost::runner::execute_test_suite(&options).await?;
            super::suite_presenter::present_suite_outcome(&options, &outcome).await?;

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
            rupost::mock::run_server_from_file(&args.file, args.port, !args.no_cors).await?;
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
            let mut replayer = rupost::runner::ReplayExecutor::new(args.target, args.verbose)?;
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
                snapshot,
                json,
                format,
            } => {
                if let Some(snapshot_path) = snapshot {
                    rupost::ai::triage::run_snapshot_triage(&snapshot_path, json, &format).await?;
                } else if let Some(report_path) = report_file {
                    rupost::ai::triage::run_ai_triage_from_file(&report_path, json, &format)
                        .await?;
                }
            }
            AiCommands::TriageSnapshot {
                snapshot,
                json,
                format,
            } => {
                rupost::ai::triage::run_snapshot_triage(&snapshot, json, &format).await?;
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
        Some(Commands::Plugin {
            allow_project_plugins,
            command,
        }) => {
            super::plugin_route::handle_plugin_command(command, allow_project_plugins)?;
        }
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
                // 03 §6.1 双模路由裁决序：内置 > 插件 > HTTP 方法 > 未知 exit 2。
                // 先从 cli.args 剥离 rupost 路由旗标再查表，确保前置/后置均可达插件命令。
                let parsed = super::plugin_route::parse_routing_args(&cli.args)?;
                let builtins = super::plugin_route::builtin_subcommand_names();
                let registry =
                    super::plugin_route::discover_for_routing(parsed.allow_project_plugins);
                match super::plugin_route::route(&parsed.passthrough, &builtins, &registry) {
                    super::plugin_route::RoutingDecision::PluginCommand(exe_path, name, rest) => {
                        let code = super::plugin_exec::launch_plugin_command(
                            &registry,
                            &name,
                            &exe_path,
                            &rest,
                            &parsed.allow_env,
                        )
                        .await;
                        std::process::exit(code);
                    }
                    super::plugin_route::RoutingDecision::Unknown(command) => {
                        eprintln!(
                            "error: 未知命令 '{command}'（内置子命令见 --help；插件命令需先安装 rupost-plugin-<name> 静态 manifest）"
                        );
                        if !registry.commands.is_empty() {
                            eprintln!("可用插件命令：");
                            for line in super::plugin_route::render_extensions(&registry) {
                                eprintln!("{line}");
                            }
                        }
                        std::process::exit(2);
                    }
                    super::plugin_route::RoutingDecision::Inline => {
                        let mut params = RunParams::from_cli(&cli);
                        params.args = parsed.passthrough;
                        runner::run(params).await?;
                    }
                }
            }
        }
    }
    Ok(())
}
