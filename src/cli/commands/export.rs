//! `rupost export`（别名 exp）：命令导出插件参数。

use clap::Parser;

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

    /// Suppress redaction audit footer on stderr
    #[arg(short, long)]
    pub quiet: bool,
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
            quiet: self.quiet,
        }
    }
}
