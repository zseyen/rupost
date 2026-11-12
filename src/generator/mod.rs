pub mod dynamic;
pub mod http;
pub mod trace;

use crate::Result;
pub use crate::generator::http::HttpGenerator;
use crate::history::selector::{self, SelectionStrategy};
use crate::history::storage::get_storage;
use std::fs;

#[derive(Debug, Clone)]
pub struct GenerateOptions {
    pub output_file: String,
    pub last: usize,
    pub interactive: bool,
    /// 将请求 origin 改写为 `{{base_url}}`，使生成文件可跨环境运行
    pub templatize_host: bool,
    /// E-3：保留明文凭据（T2 溯源不命中时免手工回填 token）
    pub keep_credentials: bool,
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

    let (content, variables) = HttpGenerator::generate_with_variables_ex(
        &entries,
        opts.templatize_host,
        opts.keep_credentials,
    )?;
    // P0-2（review 修复）：keep_credentials 产物含明文凭据——0600 原子写 + 真实警告
    if opts.keep_credentials {
        crate::security::fs::write_private_file_atomic(
            std::path::Path::new(&opts.output_file),
            content.as_bytes(),
        )?;
        eprintln!(
            "[!] {} 含明文凭据（--keep-credentials）——文件已 0600，严禁提交仓库或外传",
            opts.output_file
        );
    } else {
        fs::write(&opts.output_file, content)?;
    }
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
