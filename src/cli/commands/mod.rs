//! 子命令参数定义（解耦提案 v2 Phase 4）：每个变体一个文件，Args 结构体承载参数。
//! CLI 表面契约由 cli/mod.rs 的表驱动测试与表面契约测试锁定，迁移不得漂移。

mod ai;
mod diagnose;
mod export;
mod generate;
mod history;
mod import;
mod init;
mod mock;
mod replay;
mod test;
mod tui;
mod web;

pub use ai::AiCommands;
pub use diagnose::DiagnoseArgs;
pub use export::ExportArgs;
pub use generate::GenerateArgs;
pub use history::HistoryCommands;
pub use import::{ImportCommands, ImportCurlArgs};
pub use init::InitArgs;
pub use mock::MockArgs;
pub use replay::ReplayArgs;
pub use test::TestArgs;
pub use tui::TuiArgs;
pub use web::WebArgs;

use clap::Subcommand;

#[derive(Subcommand)]
pub enum Commands {
    /// Run requests from a file
    #[command(alias = "t")]
    Test(TestArgs),

    /// Manage request history
    #[command(alias = "h")]
    History {
        #[command(subcommand)]
        command: HistoryCommands,
    },

    /// Generate test file from history
    #[command(alias = "g")]
    Generate(GenerateArgs),

    /// Diagnose network connectivity and TLS status for a URL
    #[command(alias = "d")]
    Diagnose(DiagnoseArgs),

    /// Start a local Mock server based on a snapshot/config file
    #[command(alias = "m")]
    Mock(MockArgs),

    /// Initialize configuration or template files in the current directory (aliases: template)
    #[command(alias = "i", alias = "template")]
    Init(InitArgs),

    /// Replay HTTP requests from a snapshot file
    #[command(alias = "r")]
    Replay(ReplayArgs),

    /// Export requests from .http/.md files to external CLI commands (e.g. curl, httpie, wget)
    #[command(alias = "exp")]
    Export(ExportArgs),

    /// 启动活文档工作台 Web 服务
    #[command(alias = "w")]
    Web(WebArgs),

    /// AI assisted authoring commands
    Ai {
        #[command(subcommand)]
        command: AiCommands,
    },

    /// Import external request formats into rupost files (curl → .http)
    Import {
        #[command(subcommand)]
        command: ImportCommands,
    },

    /// Start interactive terminal UI mode
    Tui(TuiArgs),
}
