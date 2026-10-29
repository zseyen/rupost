pub mod app;
pub mod event;
pub mod files;
pub mod handlers;
pub mod ports;
pub mod state;
pub mod theme;
pub mod ui;

/// TUI 启动选项：变量上下文来源（与 CLI test 子命令同源）
#[derive(Debug, Clone, Default)]
pub struct TuiOptions {
    /// rupost.toml 中的环境名（--env）
    pub env: Option<String>,
    /// CLI 变量覆盖（--var key=value，优先级最高）
    pub vars: Vec<String>,
    /// 局部环境文件路径（--env-file；缺省自动加载 .env）
    pub env_file: Option<String>,
}

/// TUI 模式入口点
pub async fn run(initial_file: Option<String>, options: TuiOptions) -> crate::Result<()> {
    app::run(initial_file, options).await
}
