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

    /// Start interactive terminal UI mode
    Tui {
        /// Optional test file path to pre-load and preview in TUI
        #[arg(value_name = "FILE")]
        file: Option<String>,
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
}

#[derive(Parser, Debug, Clone, PartialEq, Eq)]
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

impl RunParams {
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
}
