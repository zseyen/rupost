//! `rupost generate`（别名 g）：历史 → .http 生成参数。

use clap::Parser;

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

    /// E-3：保留明文凭据（T2 溯源不命中时免手工回填 token；文件请 0600 并妥善保管）
    #[arg(long)]
    pub keep_credentials: bool,

    /// Rewrite request origin as {{base_url}} so the file runs across environments
    #[arg(long = "templatize-host")]
    pub templatize_host: bool,
}
