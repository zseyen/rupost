pub mod app;
pub mod event;
pub mod state;
pub mod ui;

/// TUI 模式入口点
pub async fn run(initial_file: Option<String>) -> crate::Result<()> {
    app::run(initial_file).await
}
