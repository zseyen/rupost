use std::process::Command;

#[test]
fn test_logger_level_filtering() {
    let bin_path = env!("CARGO_BIN_EXE_rupost");

    // 1. 测试 RUST_LOG=warn 时，不应该有 debug 级的 "Parsing command line arguments" 和 "Executing HTTP request" 输出
    let output_warn = Command::new(bin_path)
        .arg("http://127.0.0.1:0") // 传递一个无效的本地端口使其立即快速失败，避免真正的网络等待
        .env("RUST_LOG", "warn")
        .output()
        .expect("failed to execute process");

    let stderr_warn = String::from_utf8_lossy(&output_warn.stderr);
    assert!(
        !stderr_warn.contains("Parsing command line arguments"),
        "stderr should not contain debug logs in warn level, got: {}",
        stderr_warn
    );
    assert!(
        !stderr_warn.contains("Executing HTTP request"),
        "stderr should not contain executing logs in warn level, got: {}",
        stderr_warn
    );

    // 2. 测试 RUST_LOG=debug 时，应该有 debug 级的日志输出
    let output_debug = Command::new(bin_path)
        .arg("http://127.0.0.1:0")
        .env("RUST_LOG", "debug")
        .output()
        .expect("failed to execute process");

    let stderr_debug = String::from_utf8_lossy(&output_debug.stderr);
    assert!(
        stderr_debug.contains("Parsing command line arguments"),
        "stderr should contain debug logs in debug level, got: {}",
        stderr_debug
    );
    assert!(
        stderr_debug.contains("Executing HTTP request"),
        "stderr should contain executing logs in debug level, got: {}",
        stderr_debug
    );
}
