use tracing_subscriber::{EnvFilter, fmt};

/// 获取默认的日志级别
///
/// - 在 Debug 模式（开启了 debug_assertions）下默认使用 debug 级别
/// - 在 Release 模式下默认使用 warn 级别
fn get_default_level() -> &'static str {
    if cfg!(debug_assertions) {
        "warn,rupost=debug"
    } else {
        "warn"
    }
}

/// 初始化日志系统
///
/// 支持通过 RUST_LOG 环境变量控制日志级别。
/// 默认级别依据编译模式自适应：Debug 模式下为 warn,rupost=debug，Release 模式下为 warn。
///
/// 示例:
/// - RUST_LOG=debug cargo run
/// - RUST_LOG=trace cargo run
pub fn init_logger() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(get_default_level()));

    // 使用 try_init 替代 init，防止单元测试并发运行时重复初始化导致 Panic
    let _ = fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .with_writer(std::io::stderr)
        .try_init();

    tracing::debug!("Logger initialized");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_default_level() {
        let level = get_default_level();
        #[cfg(debug_assertions)]
        assert_eq!(level, "warn,rupost=debug");
        #[cfg(not(debug_assertions))]
        assert_eq!(level, "warn");
    }
}
