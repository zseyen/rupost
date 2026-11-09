//! `rupost replay`（别名 r）：快照重放参数。

use clap::Parser;

#[derive(Parser, Debug)]
pub struct ReplayArgs {
    /// Path to the snapshot JSON file to replay
    #[arg(required = true, value_name = "FILE")]
    pub file: String,

    /// Override base URL target (e.g. http://localhost:8080)
    #[arg(short, long, value_name = "URL")]
    pub target: Option<String>,

    /// Show detailed request/response details
    #[arg(short, long)]
    pub verbose: bool,

    /// Additional leaf key names to exclude from body comparison (comma-separated)
    #[arg(long, value_name = "KEYS")]
    pub ignore_fields: Option<String>,

    /// Disable body comparison (status-code-only semantics)
    #[arg(long)]
    pub status_only: bool,

    /// Explain each failed entry in place (rule baseline + AI attribution when configured)
    #[arg(long, visible_alias = "ai-explain")]
    pub explain: bool,
}
