//! 集成测试进程级隔离公共设施。
//!
//! 引用方式：在需要执行真实请求的集成测试文件顶部加 `mod common;`。
//! `#[ctor]` 在测试二进制 main 之前执行——测试线程由 harness 并行启动，
//! 逐测试设置存在竞态，必须 pre-main 一次性完成。

#[allow(dead_code)]
pub mod mock_plugin;

// 测试隔离注记（审查批二）：进程级环境突变（set_var/set_current_dir）在并行
// harness 下与同进程测试竞争——现有高危点均为"单测试独占二进制"或已持本地锁
//（template_test CWD_MUTATION_LOCK）。新增突变型测试时必须就近持锁或单列二进制。

#[ctor::ctor]
fn isolate_history_recording() {
    rupost::history::recorder::disable_history_recording();
}
