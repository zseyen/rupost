//! `rupost mock`（别名 m）：本地 Mock 服务器参数。

use clap::Parser;

#[derive(Parser, Debug)]
pub struct MockArgs {
    /// Path to the snapshot or mock configuration JSON file
    #[arg(required = true, value_name = "FILE")]
    pub file: String,

    /// Port to bind the mock server to
    #[arg(short, long, default_value = "9000")]
    pub port: u16,

    /// Disable smart CORS (preflight 204 + Origin echo) — for explicit OPTIONS mocks
    #[arg(long)]
    pub no_cors: bool,
}
