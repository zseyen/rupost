//! 集成测试进程级隔离公共设施。
//!
//! 引用方式：在需要执行真实请求的集成测试文件顶部加 `mod common;`。
//! `#[ctor]` 在测试二进制 main 之前执行——测试线程由 harness 并行启动，
//! 逐测试设置存在竞态，必须 pre-main 一次性完成。

/// 静音历史落盘：`http_runner` 对每次请求记历史，不静音则测试流量会追加进
/// 开发者真实 `.rupost/history.jsonl`（见 `history::recorder` 模块注释）。
/// 历史功能的专项测试（history_integration_test）自带 env 隔离，不引用本模块。
#[ctor::ctor]
fn isolate_history_recording() {
    rupost::history::recorder::disable_history_recording();
}
