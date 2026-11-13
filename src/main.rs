mod cli;

use rupost::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化 Rustls 默认 CryptoProvider 解决 wss:// 协议协商崩溃问题
    let _ = rustls::crypto::ring::default_provider().install_default();

    // 初始化日志系统
    rupost::logger::init_logger();

    // 全部命令派发下沉 cli.rs（本文件保持无业务逻辑，< 20 行）。
    // 退出码三值契约归位（01 §2.1，M6-5）：AST 语法解析错误 exit 2；
    // WP-4 复现包工件创建失败同样 exit 2（工件 I/O 故障不得掩盖测试结果，
    // 也区别于测试失败的 exit 1）；其余执行错误维持 exit 1；clap 参数错误已由 clap 自身 exit 2。
    if let Err(e) = cli::dispatch(cli::parse_cli()).await {
        let code = if matches!(
            e,
            rupost::RupostError::ParseError(_) | rupost::RupostError::IncidentBundleError(_)
        ) {
            2
        } else {
            1
        };
        eprintln!("[ERROR] {}", e.to_user_friendly_string());
        std::process::exit(code);
    }
    Ok(())
}
