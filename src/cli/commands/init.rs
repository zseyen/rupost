//! `rupost init`（别名 i/template）：脚手架初始化参数。

use clap::Parser;

#[derive(Parser, Debug)]
pub struct InitArgs {
    /// Type of template (e.g., config, sse)
    #[arg(default_value = "config")]
    pub r#type: String,

    /// Output file path (default depends on template type)
    #[arg(short, long)]
    pub output: Option<String>,

    /// Force overwrite existing files without prompting
    #[arg(short, long)]
    pub force: bool,

    /// List all available templates
    #[arg(short, long)]
    pub list: bool,
}
