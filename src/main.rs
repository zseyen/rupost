mod cli;

use clap::Parser;
use cli::{Cli, Commands};
fn main() {
    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Test { path }) => println!("Test: {}", path),
        None => {
            if cli.args.is_empty() {
                println!("No command provided");
            } else {
                cli::run(cli.args);
            }
        }
    }
}
