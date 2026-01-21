mod cli;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands};
#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Test { path }) => println!("Test: {}", path),
        None => {
            if cli.args.is_empty() {
                println!("No command provided");
            } else {
                cli::run(cli.args).await?;
            }
        }
    }
    Ok(())
}
