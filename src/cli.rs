mod parser;

use clap::{Parser, Subcommand};
use rupost::Result;
use rupost::http::Response;
use rupost::runner::TestExecutor;
use rupost::utils::{ResponseFormat, ResponseFormatter};
use rupost::variable::VariableContext;
use tracing::{debug, error};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// 可选参数用于默认运行(curl/httpie 风格)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,

    /// Disable automatic cookie handling for CLI requests
    #[arg(long, global = true)]
    pub no_cookies: bool,

    /// File path for persistent cookie storage for CLI requests
    #[arg(long, global = true, value_name = "FILE")]
    pub cookie_file: Option<String>,

    /// Enable detailed network diagnostics timing
    #[arg(long, global = true)]
    pub debug: bool,

    /// Enable detailed network diagnostics only on failure
    #[arg(long, global = true)]
    pub debug_on_failure: bool,

    /// Default scheme when URL does not contain one
    #[arg(long, global = true, default_value = "http")]
    pub default_scheme: String,

    /// Environment name (e.g., dev, staging, prod) — resolves `--cookie-file`
    /// to an environment-suffixed file for inline requests, mirroring `test`
    #[arg(long, global = true, value_name = "NAME")]
    pub env: Option<String>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Run requests from a file
    #[command(alias = "t")]
    Test {
        /// Paths to .http/.md files or directories containing them
        #[arg(required = true, value_name = "PATHS")]
        paths: Vec<String>,

        /// 执行模式 (sequential 或 parallel)
        #[arg(long, default_value = "sequential")]
        mode: String,

        /// 并行模式下的并发数
        #[arg(long, default_value = "4")]
        concurrency: usize,

        /// 报告输出格式 (terminal 或 json)
        #[arg(long, default_value = "terminal")]
        report: String,

        /// 将 JSON 格式测试报告额外写入指定文件（不干扰终端输出）
        #[arg(long, value_name = "FILE")]
        report_file: Option<String>,

        /// 对每个失败请求输出原位原因分析与修复建议（兼容 --ai-explain 与 RUPOST_AI_EXPLAIN=1）
        #[arg(long, visible_alias = "ai-explain")]
        explain: bool,

        /// 在批量测试汇总后输出失败根因聚类分诊报告
        #[arg(long)]
        triage: bool,

        /// 遇到第一个失败文件时立即停止
        #[arg(long)]
        fail_fast: bool,

        /// Environment name (e.g., dev, staging, prod)
        #[arg(short, long)]
        env: Option<String>,

        /// Variable overrides (key=value)
        #[arg(long, value_name = "KEY=VALUE")]
        var: Vec<String>,

        /// Path to a local environment file (e.g., .env)
        #[arg(long, value_name = "FILE")]
        env_file: Option<String>,

        /// Show detailed request/response information
        #[arg(short, long)]
        verbose: bool,

        /// Disable automatic cookie handling
        #[arg(long)]
        no_cookies: bool,

        /// File path for persistent cookie storage
        #[arg(long, value_name = "FILE")]
        cookie_file: Option<String>,

        /// File path to save HTTP request/response snapshot
        #[arg(long, value_name = "FILE")]
        save_snapshot: Option<String>,

        /// 只运行单个请求：文件内序号 (3) 或 @name (@login)；也可用 path#3 片段语法
        #[arg(long, value_name = "N|@NAME")]
        only: Option<String>,
    },

    /// Manage request history
    #[command(alias = "h")]
    History {
        #[command(subcommand)]
        command: HistoryCommands,
    },

    /// Generate test file from history
    #[command(alias = "g")]
    Generate(GenerateArgs),
    /// Diagnose network connectivity and TLS status for a URL
    #[command(alias = "d")]
    Diagnose {
        /// Target URL to diagnose (e.g. https://example.com)
        #[arg(required = true)]
        url: String,

        /// Format output (terminal or json)
        #[arg(long, default_value = "terminal")]
        report: String,
    },

    /// Start a local Mock server based on a snapshot/config file
    #[command(alias = "m")]
    Mock {
        /// Path to the snapshot or mock configuration JSON file
        #[arg(required = true, value_name = "FILE")]
        file: String,

        /// Port to bind the mock server to
        #[arg(short, long, default_value = "9000")]
        port: u16,
    },
    /// Initialize configuration or template files in the current directory (aliases: template)
    #[command(alias = "i", alias = "template")]
    Init {
        /// Type of template (e.g., config, sse)
        #[arg(default_value = "config")]
        r#type: String,

        /// Output file path (default depends on template type)
        #[arg(short, long)]
        output: Option<String>,

        /// Force overwrite existing files without prompting
        #[arg(short, long)]
        force: bool,

        /// List all available templates
        #[arg(short, long)]
        list: bool,
    },

    /// Replay HTTP requests from a snapshot file
    #[command(alias = "r")]
    Replay {
        /// Path to the snapshot JSON file to replay
        #[arg(required = true, value_name = "FILE")]
        file: String,

        /// Override base URL target (e.g. http://localhost:8080)
        #[arg(short, long, value_name = "URL")]
        target: Option<String>,

        /// Show detailed request/response details
        #[arg(short, long)]
        verbose: bool,
    },

    /// Export requests from .http/.md files to external CLI commands (e.g. curl, httpie, wget)
    #[command(alias = "exp")]
    Export(ExportArgs),

    /// AI assisted authoring commands
    Ai {
        #[command(subcommand)]
        command: AiCommands,
    },

    /// Import external request formats into rupost files (curl → .http)
    Import {
        #[command(subcommand)]
        command: ImportCommands,
    },

    /// Start interactive terminal UI mode
    Tui {
        /// Optional test file path to pre-load and preview in TUI
        #[arg(value_name = "FILE")]
        file: Option<String>,

        /// Environment name in rupost.toml (e.g., dev, staging)
        #[arg(long)]
        env: Option<String>,

        /// Variable overrides (key=value)
        #[arg(long, value_name = "KEY=VALUE")]
        var: Vec<String>,

        /// Path to a local environment file (e.g. .env)
        #[arg(long, value_name = "FILE")]
        env_file: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum ImportCommands {
    /// Import a curl command into an executable .http request block
    Curl(ImportCurlArgs),
}

#[derive(Parser, Debug)]
pub struct ImportCurlArgs {
    /// curl 命令：整体引号包裹，或直接粘贴为多参数（自动空格拼接）
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "CURL"
    )]
    pub curl: Vec<String>,

    /// 写入 .http 文件（已存在则拒绝）；缺省打印 stdout
    #[arg(short, long, value_name = "FILE")]
    pub output: Option<String>,

    /// origin 模板化为 {{base_url}}（跨环境运行）
    #[arg(long)]
    pub templatize_host: bool,
}

impl ImportCurlArgs {
    pub fn into_request(self) -> rupost::importer::curl::CurlImportRequest {
        // trailing_var_arg 拦截（AGENTS §5 同源陷阱）：位置参数之后的 -o/--output/--out
        // 与 --templatize-host 属 rupost 侧开关——从 CURL 序列中摘出。
        // 已知取舍：curl 自身的 -o（响应落盘）语义在此被覆盖——devtools 复制的
        // curl 不含 -o，真实冲突罕见；现有测试与 CHANGELOG 契约依赖本拦截，保留。
        let mut output = self.output;
        let mut templatize_host = self.templatize_host;
        let mut kept: Vec<String> = Vec::with_capacity(self.curl.len());
        let mut it = self.curl.into_iter().peekable();
        while let Some(tok) = it.next() {
            match tok.as_str() {
                "-o" | "--output" | "--out" => {
                    if let Some(v) = it.next() {
                        output = Some(v);
                    }
                }
                "--templatize-host" => templatize_host = true,
                other => kept.push(other.to_string()),
            }
        }

        rupost::importer::curl::CurlImportRequest {
            raw: kept.join(" "),
            output,
            templatize_host,
        }
    }
}

#[derive(Subcommand)]
pub enum AiCommands {
    /// Translate natural language expectations into standard @assert lines
    Assert {
        /// Natural language description of the expected behavior
        text: String,

        /// Attach the last N history entries (redacted) as response context
        #[arg(long, value_name = "N", conflicts_with = "response")]
        last: Option<usize>,

        /// Attach a response body file (redacted) as context
        #[arg(long, value_name = "FILE", conflicts_with = "last")]
        response: Option<String>,

        /// Disable context redaction (values are sent as-is to the AI endpoint)
        #[arg(long)]
        no_mask: bool,

        /// Print plain assertion lines only (machine-friendly)
        #[arg(long)]
        raw: bool,
    },

    /// Cluster failed requests from a JSON test report into root-cause groups
    Triage {
        /// Path to the JSON report generated by `rupost test --report-file`
        #[arg(value_name = "REPORT_JSON")]
        report_file: String,

        /// Output machine-readable JSON triage report
        #[arg(long)]
        json: bool,

        /// Output format (markdown or json)
        #[arg(long, default_value = "markdown")]
        format: String,
    },
    /// Expand in-file `# @ai-assert` directives into standard @assert lines (B2: AI failure leaves file untouched)
    Resolve {
        /// Path to the .http/.md file containing @ai-assert directives
        #[arg(value_name = "FILE")]
        file: String,

        /// Print proposed expansion without writing
        #[arg(long)]
        dry_run: bool,
    },

    /// Distill recent history into a semantic regression test file (AI-enhanced, degrades to mechanical baseline)
    Bootstrap {
        /// Asset source (MVP: "history")
        #[arg(long, default_value = "history")]
        from: String,

        /// Take the last N history entries
        #[arg(long, default_value_t = 10)]
        last: usize,

        /// Only include entries whose URL contains this prefix
        #[arg(long, value_name = "PREFIX")]
        filter: Option<String>,

        /// Output .http file (refuses to overwrite)
        #[arg(short, long, value_name = "FILE")]
        output: String,

        /// Skip AI enhancement (emit mechanical baseline only)
        #[arg(long)]
        no_ai: bool,

        /// Template request origin as {{base_url}}
        #[arg(long)]
        templatize_host: bool,
    },
}

#[derive(Subcommand)]
pub enum HistoryCommands {
    /// List request history
    #[command(alias = "l")]
    List {
        /// Limit the number of entries
        #[arg(long, default_value = "20")]
        limit: usize,

        /// Show latest entries first (Effective mainly for UI display)
        #[arg(short, long)]
        reverse: bool,
    },

    /// Export history entries to a snapshot file
    #[command(alias = "e")]
    Export {
        /// Number of recent runs to include
        #[arg(long, default_value = "1")]
        last: usize,

        /// Output path for the snapshot JSON file
        #[arg(short, long, required = true)]
        output: String,
    },

    /// Show detailed response and body/frames for a specific history entry
    #[command(alias = "s")]
    Show {
        /// History entry number (1-indexed, 1 is the newest) or UUID
        #[arg(required = true)]
        target: String,
    },

    /// Prune long connection logs manually
    #[command(alias = "p")]
    Prune {
        /// Prune logs modified older than X days
        #[arg(long, default_value = "7")]
        days: u64,

        /// Prune logs if log directory exceeds Y megabytes
        #[arg(long, default_value = "100")]
        max_size: usize,
    },

    /// Clear all long connection logs physically
    #[command(alias = "c")]
    Clear {
        /// Force delete without interactive confirmation
        #[arg(short, long)]
        yes: bool,

        /// Clear main history.jsonl db file as well
        #[arg(long)]
        all: bool,
    },
}

#[derive(Parser, Debug)]
pub struct GenerateArgs {
    /// Output file path
    pub output_file: String,

    /// Number of recent requests to include
    #[arg(short, long, default_value = "1")]
    pub last: usize,

    /// Interactive selection mode
    #[arg(short, long)]
    pub interactive: bool,

    /// Rewrite request origin as {{base_url}} so the file runs across environments
    #[arg(long = "templatize-host")]
    pub templatize_host: bool,
}

#[derive(Parser, Debug, Clone, PartialEq, Eq, Default)]
pub struct ExportArgs {
    /// Target export format (e.g. curl, httpie, wget)
    #[arg(value_name = "TARGET", required_unless_present = "list")]
    pub target: Option<String>,

    /// Paths to .http/.md files or directories
    #[arg(value_name = "PATHS", required_unless_present = "list")]
    pub paths: Vec<String>,

    /// Environment name (e.g. dev, staging, prod)
    #[arg(short, long)]
    pub env: Option<String>,

    /// Variable overrides (key=value)
    #[arg(long, value_name = "KEY=VALUE")]
    pub var: Vec<String>,

    /// Path to a local environment file (e.g. .env)
    #[arg(long, value_name = "FILE")]
    pub env_file: Option<String>,

    /// Path to cookie store file
    #[arg(long, value_name = "FILE")]
    pub cookie_file: Option<String>,

    /// Filter and export only the request matching the given @name
    #[arg(short, long)]
    pub name: Option<String>,

    /// Output in single-line compact format
    #[arg(short, long)]
    pub compact: bool,

    /// Redact sensitive credentials in headers and query params
    #[arg(long)]
    pub redact: bool,

    /// Save output directly to file
    #[arg(short, long, value_name = "FILE")]
    pub output: Option<String>,

    /// Force overwrite output file if it already exists
    #[arg(short, long)]
    pub force: bool,

    /// List all available export plugins
    #[arg(short, long)]
    pub list: bool,
}

impl ExportArgs {
    pub fn into_run_options(
        self,
        default_env: Option<String>,
        default_cookie_file: Option<String>,
        default_scheme: String,
    ) -> rupost::exporter::ExportRunOptions {
        rupost::exporter::ExportRunOptions {
            target: self.target,
            paths: self.paths,
            env: self.env.or(default_env),
            var: self.var,
            env_file: self.env_file,
            cookie_file: self.cookie_file.or(default_cookie_file),
            default_scheme,
            name: self.name,
            compact: self.compact,
            redact: self.redact,
            output: self.output.map(std::path::PathBuf::from),
            force: self.force,
            list: self.list,
        }
    }
}

/// Parameters for inline CLI single-request execution mode
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunParams {
    pub args: Vec<String>,
    pub no_cookies: bool,
    pub cookie_file: Option<String>,
    pub env: Option<String>,
    pub default_scheme: String,
    pub debug: bool,
    pub debug_on_failure: bool,
}

#[allow(dead_code)]
impl Default for RunParams {
    fn default() -> Self {
        Self {
            args: Vec::new(),
            no_cookies: false,
            cookie_file: None,
            env: None,
            default_scheme: "http".to_string(),
            debug: false,
            debug_on_failure: false,
        }
    }
}

#[allow(dead_code)]
impl RunParams {
    /// Create a new `RunParams` with the specified arguments and default configuration
    pub fn new(args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            args: args.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }

    /// Construct RunParams directly from parsed CLI arguments
    pub fn from_cli(cli: &Cli) -> Self {
        Self {
            args: cli.args.clone(),
            no_cookies: cli.no_cookies,
            cookie_file: cli.cookie_file.clone(),
            env: cli.env.clone(),
            default_scheme: cli.default_scheme.clone(),
            debug: cli.debug,
            debug_on_failure: cli.debug_on_failure,
        }
    }

    /// Configure target environment
    pub fn with_env(mut self, env: impl Into<String>) -> Self {
        self.env = Some(env.into());
        self
    }

    /// Configure default URL scheme
    pub fn with_default_scheme(mut self, scheme: impl Into<String>) -> Self {
        self.default_scheme = scheme.into();
        self
    }

    /// Configure cookie storage file path
    pub fn with_cookie_file(mut self, path: impl Into<String>) -> Self {
        self.cookie_file = Some(path.into());
        self
    }

    /// Configure whether automatic cookies handling is disabled
    pub fn with_no_cookies(mut self, no_cookies: bool) -> Self {
        self.no_cookies = no_cookies;
        self
    }

    /// Configure debug network diagnostics
    pub fn with_debug(mut self, debug: bool) -> Self {
        self.debug = debug;
        self
    }

    /// Configure debug network diagnostics on failure
    pub fn with_debug_on_failure(mut self, debug_on_failure: bool) -> Self {
        self.debug_on_failure = debug_on_failure;
        self
    }
}

struct CliRunner {
    formatter: ResponseFormatter,
    executor: TestExecutor,
    default_scheme: String,
}

impl CliRunner {
    fn new(params: &RunParams) -> Result<Self> {
        let executor = if params.no_cookies {
            TestExecutor::new()
        } else if let Some(path) = &params.cookie_file {
            // 与测试套件路径一致：cookie 文件按环境隔离（cookies_dev.json 等）
            let resolved = rupost::middleware::resolve_cookie_path(
                std::path::PathBuf::from(path),
                params.env.as_deref(),
            )?;
            TestExecutor::with_cookies(resolved)?
        } else {
            TestExecutor::with_ephemeral_cookies()
        }
        .with_debug(params.debug)
        .with_debug_on_failure(params.debug_on_failure);

        Ok(Self {
            formatter: ResponseFormatter::new(ResponseFormat::Verbose),
            executor,
            default_scheme: params.default_scheme.clone(),
        })
    }

    async fn run(self, args: Vec<String>) -> Result<()> {
        debug!("Parsing command line arguments");
        let parsed_request = parser::parse_args(&args)?;

        // Setup empty context for CLI run
        let mut context = VariableContext::new();
        context.insert("__default_scheme", &self.default_scheme);

        debug!(url = %parsed_request.url, method = ?parsed_request.method_or_default(), "Executing HTTP request");

        // Execute with source="cli"
        let result = self
            .executor
            .execute_one(parsed_request, 1, &mut context, Some("cli".to_string()))
            .await;

        if result.success {
            if let Some(response) = result.response {
                self.format_response(response);
            }
        } else {
            error!("Request failed: {}", result.error.unwrap_or_default());
        }

        Ok(())
    }

    fn format_response(&self, response: Response) {
        match self.formatter.format(&response) {
            Ok(output) => println!("{}", output),
            Err(e) => error!("Failed to format response: {}", e),
        }
    }
}

pub async fn run(params: RunParams) -> Result<()> {
    let args = params.args.clone();
    let runner = CliRunner::new(&params)?;
    runner.run(args).await
}

/// 命令派发总入口（自 main.rs 下沉：本文件承载完整 CLI 语义，main.rs 保持无业务逻辑）
pub async fn dispatch(cli: Cli) -> Result<()> {
    use colored::Colorize;

    match cli.command {
        Some(Commands::Test {
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
            only,
        }) => {
            // --explain 兼容环境变量开关（flag 未传时 RUPOST_AI_EXPLAIN=1/true/yes/on 生效）
            let explain_enabled = explain
                || std::env::var("RUPOST_AI_EXPLAIN").is_ok_and(|v| {
                    matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
                });
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
                only,
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
        Some(Commands::Tui {
            file,
            env,
            var,
            env_file,
        }) => {
            rupost::tui::run(
                file,
                rupost::tui::TuiOptions {
                    env,
                    vars: var,
                    env_file,
                },
            )
            .await?;
        }
        None => {
            if cli.args.is_empty() {
                tracing::error!("No command provided");
                std::process::exit(1);
            } else {
                run(RunParams::from_cli(&cli)).await?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_export_args_and_run_params_mapping() {
        let input = vec![
            "rupost",
            "--env",
            "prod",
            "--default-scheme",
            "http",
            "export",
            "curl",
            "tests/sample.http",
            "--compact",
            "--redact",
        ];
        let cli = Cli::try_parse_from(&input).expect("parse failed");
        assert_eq!(cli.env.as_deref(), Some("prod"));
        assert_eq!(cli.default_scheme, "http");

        match cli.command {
            Some(Commands::Export(args)) => {
                assert_eq!(args.target.as_deref(), Some("curl"));
                assert_eq!(args.paths, vec!["tests/sample.http"]);
                assert!(args.compact);
                assert!(args.redact);

                let run_opts = args.into_run_options(
                    cli.env.clone(),
                    cli.cookie_file.clone(),
                    cli.default_scheme.clone(),
                );
                assert_eq!(run_opts.target.as_deref(), Some("curl"));
                assert_eq!(run_opts.env.as_deref(), Some("prod"));
                assert_eq!(run_opts.default_scheme, "http");
                assert!(run_opts.compact);
                assert!(run_opts.redact);
            }
            _ => panic!("Expected Commands::Export"),
        }

        let inline_input = vec!["rupost", "--debug", "GET", "https://api.example.com"];
        let inline_cli = Cli::try_parse_from(&inline_input).expect("inline parse failed");
        let params = RunParams::from_cli(&inline_cli);
        assert_eq!(params.args, vec!["GET", "https://api.example.com"]);
        assert!(params.debug);
        assert!(!params.debug_on_failure);
    }

    #[test]
    fn test_run_params_builder_and_default() {
        // 1. 测试默认值规范
        let default_params = RunParams::default();
        assert!(default_params.args.is_empty());
        assert!(!default_params.no_cookies);
        assert_eq!(default_params.cookie_file, None);
        assert_eq!(default_params.env, None);
        assert_eq!(default_params.default_scheme, "http");
        assert!(!default_params.debug);
        assert!(!default_params.debug_on_failure);

        // 2. 测试 new 构造器与链式 Builder
        let custom_params = RunParams::new(["POST", "http://127.0.0.1:8080/api"])
            .with_env("staging")
            .with_default_scheme("https")
            .with_cookie_file("/tmp/cookies.json")
            .with_no_cookies(true)
            .with_debug(true)
            .with_debug_on_failure(true);

        assert_eq!(
            custom_params.args,
            vec!["POST", "http://127.0.0.1:8080/api"]
        );
        assert_eq!(custom_params.env.as_deref(), Some("staging"));
        assert_eq!(custom_params.default_scheme, "https");
        assert_eq!(
            custom_params.cookie_file.as_deref(),
            Some("/tmp/cookies.json")
        );
        assert!(custom_params.no_cookies);
        assert!(custom_params.debug);
        assert!(custom_params.debug_on_failure);

        // 3. 测试 ExportArgs 默认值
        let export_args = ExportArgs::default();
        assert_eq!(export_args.target, None);
        assert!(export_args.paths.is_empty());
        assert!(!export_args.compact);
        assert!(!export_args.redact);
    }

    struct TestCase {
        name: &'static str,
        input: Vec<&'static str>,
        expected_debug: bool,
        expected_debug_on_failure: bool,
        expected_env: Option<&'static str>,
        expected_has_subcommand: bool,
        expected_args: Vec<&'static str>,
    }

    #[test]
    fn test_global_args_position_contract_table() {
        let cases = vec![
            TestCase {
                name: "pre_subcmd_debug",
                input: vec!["rupost", "--debug", "test", "suite.http"],
                expected_debug: true,
                expected_debug_on_failure: false,
                expected_env: None,
                expected_has_subcommand: true,
                expected_args: vec![],
            },
            TestCase {
                name: "post_subcmd_debug",
                input: vec!["rupost", "test", "suite.http", "--debug"],
                expected_debug: true,
                expected_debug_on_failure: false,
                expected_env: None,
                expected_has_subcommand: true,
                expected_args: vec![],
            },
            TestCase {
                name: "pre_subcmd_debug_on_failure",
                input: vec!["rupost", "--debug-on-failure", "test", "suite.http"],
                expected_debug: false,
                expected_debug_on_failure: true,
                expected_env: None,
                expected_has_subcommand: true,
                expected_args: vec![],
            },
            TestCase {
                name: "post_subcmd_debug_on_failure",
                input: vec!["rupost", "test", "suite.http", "--debug-on-failure"],
                expected_debug: false,
                expected_debug_on_failure: true,
                expected_env: None,
                expected_has_subcommand: true,
                expected_args: vec![],
            },
            TestCase {
                name: "mixed_flags_both_positions",
                input: vec![
                    "rupost",
                    "--debug",
                    "test",
                    "suite.http",
                    "--debug-on-failure",
                ],
                expected_debug: true,
                expected_debug_on_failure: true,
                expected_env: None,
                expected_has_subcommand: true,
                expected_args: vec![],
            },
            TestCase {
                name: "global_value_arg_pre",
                input: vec!["rupost", "--env", "staging", "test", "suite.http"],
                expected_debug: false,
                expected_debug_on_failure: false,
                expected_env: Some("staging"),
                expected_has_subcommand: true,
                expected_args: vec![],
            },
            TestCase {
                name: "global_value_arg_post",
                input: vec!["rupost", "test", "suite.http", "--env", "staging"],
                expected_debug: false,
                expected_debug_on_failure: false,
                expected_env: Some("staging"),
                expected_has_subcommand: true,
                expected_args: vec![],
            },
            TestCase {
                name: "inline_mode_pre_debug",
                input: vec!["rupost", "--debug", "GET", "http://example.com"],
                expected_debug: true,
                expected_debug_on_failure: false,
                expected_env: None,
                expected_has_subcommand: false,
                expected_args: vec!["GET", "http://example.com"],
            },
            TestCase {
                name: "inline_mode_post_debug_trap",
                input: vec!["rupost", "GET", "http://example.com", "--debug"],
                expected_debug: false,
                expected_debug_on_failure: false,
                expected_env: None,
                expected_has_subcommand: false,
                expected_args: vec!["GET", "http://example.com", "--debug"],
            },
        ];

        for case in cases {
            let parsed = Cli::try_parse_from(&case.input)
                .unwrap_or_else(|e| panic!("Failed to parse in case '{}': {}", case.name, e));

            assert_eq!(
                parsed.debug, case.expected_debug,
                "Case '{}' debug flag mismatch",
                case.name
            );
            assert_eq!(
                parsed.debug_on_failure, case.expected_debug_on_failure,
                "Case '{}' debug_on_failure flag mismatch",
                case.name
            );
            assert_eq!(
                parsed.env.as_deref(),
                case.expected_env,
                "Case '{}' env mismatch",
                case.name
            );
            assert_eq!(
                parsed.command.is_some(),
                case.expected_has_subcommand,
                "Case '{}' subcommand presence mismatch",
                case.name
            );
            assert_eq!(
                parsed.args, case.expected_args,
                "Case '{}' args mismatch",
                case.name
            );
        }
    }

    /// R2 验收（TUI 流式调试 PRD）：`tui` 子命令的 --env/--var/--env-file 解析
    /// 与 TuiOptions 组装映射。变量允许多次 --var 累积；file 为可选位置参数。
    #[test]
    fn test_tui_args_parse_and_options_mapping() {
        let cli = Cli::try_parse_from([
            "rupost",
            "tui",
            "suite.http",
            "--env",
            "staging",
            "--var",
            "k1=v1",
            "--var",
            "k2=v2",
            "--env-file",
            ".env.staging",
        ])
        .expect("tui args must parse");

        let Some(Commands::Tui {
            file,
            env,
            var,
            env_file,
        }) = cli.command
        else {
            panic!("expected Tui subcommand");
        };
        {
            assert_eq!(file.as_deref(), Some("suite.http"));
            assert_eq!(env.as_deref(), Some("staging"));
            assert_eq!(var, vec!["k1=v1".to_string(), "k2=v2".to_string()]);
            assert_eq!(env_file.as_deref(), Some(".env.staging"));

            // main.rs 分发处的 TuiOptions 组装映射（字段一一对应）
            let opts = rupost::tui::TuiOptions {
                env: env.clone(),
                vars: var.clone(),
                env_file: env_file.clone(),
            };
            assert_eq!(opts.env.as_deref(), Some("staging"));
            assert_eq!(opts.vars, vec!["k1=v1".to_string(), "k2=v2".to_string()]);
            assert_eq!(opts.env_file.as_deref(), Some(".env.staging"));
        }
    }

    /// R2：无参 `tui` 的默认值（file/env/env_file 均空、vars 空集）
    #[test]
    fn test_tui_defaults_without_file() {
        let cli = Cli::try_parse_from(["rupost", "tui"]).expect("bare tui must parse");
        let Some(Commands::Tui {
            file,
            env,
            var,
            env_file,
        }) = cli.command
        else {
            panic!("expected Tui subcommand");
        };
        assert!(file.is_none());
        assert!(env.is_none());
        assert!(var.is_empty());
        assert!(env_file.is_none());
    }
}
