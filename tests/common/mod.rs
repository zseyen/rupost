//! 集成测试进程级隔离公共设施。
//!
//! 引用方式：在需要执行真实请求的集成测试文件顶部加 `mod common;`。
//! `#[ctor]` 在测试二进制 main 之前执行——测试线程由 harness 并行启动，
//! 逐测试设置存在竞态，必须 pre-main 一次性完成。

//! 写测试前必读（批四，2026-11-18）：
//! 1. 本文件 `#[ctor]` 在 pre-main 静音历史落盘——**历史目录在首次落盘时固化**，
//!    事后改 RUPOST_HISTORY_DIR 无效：需要历史数据的测试必须走子进程
//!    （`CARGO_BIN_EXE_rupost` + `.env("RUPOST_HISTORY_DIR", ...)`，全库 47 处先例）。
//! 2. env 突变（set_var/remove_var）测试必须持锁（template_test CWD_MUTATION_LOCK、
//!    plugin_pipeline ENV_MUTATION_LOCK 先例）或单列二进制——并行测试共享进程 env。
//! 3. mock_plugin.rs 的 DSL 行协议：每行必须含 `=`（split_once 解析，无 `=` 整行
//!    被跳过）；`|` 字段分隔数按指令而异；stdin 只能被 READ_STDIN/CAPTURE 消费一次。

#[allow(dead_code)]
pub mod mock_plugin;

// 测试隔离注记（审查批二）：进程级环境突变（set_var/set_current_dir）在并行
// harness 下与同进程测试竞争——现有高危点均为"单测试独占二进制"或已持本地锁
//（template_test CWD_MUTATION_LOCK）。新增突变型测试时必须就近持锁或单列二进制。

#[ctor::ctor]
fn isolate_history_recording() {
    rupost::history::recorder::disable_history_recording();
}
