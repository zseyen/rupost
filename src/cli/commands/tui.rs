//! `rupost tui`：交互式终端 UI 参数。

use clap::Parser;

#[derive(Parser, Debug)]
pub struct TuiArgs {
    /// Optional test file path to pre-load and preview in TUI
    #[arg(value_name = "FILE")]
    pub file: Option<String>,

    /// Environment name in rupost.toml (e.g., dev, staging)
    #[arg(long)]
    pub env: Option<String>,

    /// Variable overrides (key=value)
    #[arg(long, value_name = "KEY=VALUE")]
    pub var: Vec<String>,

    /// Path to a local environment file (e.g. .env)
    #[arg(long, value_name = "FILE")]
    pub env_file: Option<String>,

    /// Cookie store file for persistence across runs
    #[arg(long, value_name = "FILE")]
    pub cookie_file: Option<String>,
}
