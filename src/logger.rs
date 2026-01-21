use tracing_subscriber::{EnvFilter, fmt};

/// 初始化日志系统
///
/// 支持通过 RUST_LOG 环境变量控制日志级别
/// 默认级别: info
///
/// 示例:
/// - RUST_LOG=debug cargo run
/// - RUST_LOG=trace cargo run
pub fn init_logger() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .init();

    tracing::info!("Logger initialized");
}
