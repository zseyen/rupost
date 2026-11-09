//! `rupost diagnose`（别名 d）：网络连通性诊断参数。

use clap::Parser;

#[derive(Parser, Debug)]
pub struct DiagnoseArgs {
    /// Target URL to diagnose (e.g. https://example.com)
    #[arg(required = true)]
    pub url: String,

    /// Format output (terminal or json)
    #[arg(long, default_value = "terminal")]
    pub report: String,
}
