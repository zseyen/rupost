pub mod http;

use crate::Result;
use crate::generator::http::HttpGenerator;
use crate::history::selector::{self, SelectionStrategy};
use crate::history::storage::get_storage;
use std::fs;

#[derive(Debug, Clone)]
pub struct GenerateOptions {
    pub output_file: String,
    pub last: usize,
    pub interactive: bool,
}

/// 执行从历史生成测试文件的机制层函数
pub fn run_generate(opts: GenerateOptions) -> Result<()> {
    let storage = get_storage();
    let strategy = if opts.interactive {
        SelectionStrategy::Interactive
    } else {
        SelectionStrategy::Last(opts.last)
    };

    let entries = selector::select_entries(storage, strategy)?;
    if entries.is_empty() {
        tracing::warn!("No history found or selected to generate.");
        return Ok(());
    }

    let (content, variables) = HttpGenerator::generate_with_variables(&entries)?;
    fs::write(&opts.output_file, content)?;
    println!(
        "Generated test file: {} ({} requests)",
        opts.output_file,
        entries.len()
    );
    if !variables.is_empty() {
        println!(
            "  Variables referenced: {} (provide via --var / .env / rupost.toml)",
            variables.join(", ")
        );
    }
    Ok(())
}
