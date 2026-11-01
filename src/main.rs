mod cli;

use clap::Parser;
use cli::Cli;
use rupost::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化 Rustls 默认 CryptoProvider 解决 wss:// 协议协商崩溃问题
    let _ = rustls::crypto::ring::default_provider().install_default();

    // 初始化日志系统
    rupost::logger::init_logger();

    // 全部命令派发下沉 cli.rs（本文件保持无业务逻辑，< 20 行）
    cli::dispatch(Cli::parse()).await
}
