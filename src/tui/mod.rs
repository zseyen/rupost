pub mod app;
pub mod event;
pub mod state;
pub mod ui;

/// TUI 模式入口点
pub fn run() -> crate::Result<()> {
    app::run()
}
