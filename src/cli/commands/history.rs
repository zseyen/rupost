//! `rupost history`（别名 h）：历史子命令枚举。

use clap::Subcommand;

#[derive(Subcommand)]
pub enum HistoryCommands {
    /// List request history
    #[command(alias = "l")]
    List {
        /// Limit the number of entries
        #[arg(long, default_value = "20")]
        limit: usize,

        /// Show latest entries first (Effective mainly for UI display)
        #[arg(short, long)]
        reverse: bool,
    },

    /// Export history entries to a snapshot file
    #[command(alias = "e")]
    Export {
        /// Number of recent requests to include
        #[arg(long, default_value = "1")]
        last: usize,

        /// Output path for the snapshot JSON file
        #[arg(short, long, required = true)]
        output: String,

        /// Mask headers/body secrets in the exported snapshot (opt-in)
        #[arg(long)]
        mask_secrets: bool,
    },

    /// Show detailed response and body/frames for a specific history entry
    #[command(alias = "s")]
    Show {
        /// History entry number (1-indexed, 1 is the newest) or UUID
        #[arg(required = true)]
        target: String,
    },

    /// Prune long connection logs manually
    #[command(alias = "p")]
    Prune {
        /// Prune logs modified older than X days
        #[arg(long, default_value = "7")]
        days: u64,

        /// Prune logs if log directory exceeds Y megabytes
        #[arg(long, default_value = "100")]
        max_size: usize,
    },

    /// Clear all long connection logs physically
    #[command(alias = "c")]
    Clear {
        /// Force delete without interactive confirmation
        #[arg(short, long)]
        yes: bool,

        /// Clear main history.jsonl db file as well
        #[arg(long)]
        all: bool,
    },
}
