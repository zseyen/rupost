//! 内联请求运行器（curl/httpie 风格参数对象与执行器，解耦提案 v2 Phase 4 自 cli.rs 归位）。

use crate::cli::Cli;
use crate::cli::parser;
use rupost::Result;
use rupost::http::Response;
use rupost::runner::TestExecutor;
use rupost::utils::{ResponseFormat, ResponseFormatter};
use rupost::variable::VariableContext;
use tracing::{debug, error};

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
