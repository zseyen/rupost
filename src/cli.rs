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

    /// Start interactive terminal UI mode
    Tui,
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

struct CliRunner {
    formatter: ResponseFormatter,
    executor: TestExecutor,
    default_scheme: String,
}

impl CliRunner {
    fn new(
        no_cookies: bool,
        cookie_file: Option<String>,
        default_scheme: String,
        debug: bool,
        debug_on_failure: bool,
    ) -> Result<Self> {
        let executor = if no_cookies {
            TestExecutor::new()
        } else if let Some(path) = cookie_file {
            TestExecutor::with_cookies(std::path::PathBuf::from(path))?
        } else {
            TestExecutor::with_ephemeral_cookies()
        }
        .with_debug(debug)
        .with_debug_on_failure(debug_on_failure);

        Ok(Self {
            formatter: ResponseFormatter::new(ResponseFormat::Verbose),
            executor,
            default_scheme,
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

pub async fn run(
    args: Vec<String>,
    no_cookies: bool,
    cookie_file: Option<String>,
    default_scheme: String,
    debug: bool,
    debug_on_failure: bool,
) -> Result<()> {
    let runner = CliRunner::new(
        no_cookies,
        cookie_file,
        default_scheme,
        debug,
        debug_on_failure,
    )?;
    runner.run(args).await
}
