//! 集成测试进程级隔离公共设施。
//!
//! 引用方式：在需要执行真实请求的集成测试文件顶部加 `mod common;`。
//! `#[ctor]` 在测试二进制 main 之前执行——测试线程由 harness 并行启动，
//! 逐测试设置存在竞态，必须 pre-main 一次性完成。

#[allow(dead_code)]
pub mod mock_plugin;

/// 测试隔离件②（审查批二）：进程级环境突变互斥——`set_var`/`set_current_dir`
/// 是进程级全局突变，并行 harness 下与同进程其他测试真竞争。突变型测试持锁
/// 串行；残余风险（未持锁测试恰在读窗口）以"多数测试经 ctor 静音历史、不读
/// 这些变量"为界，已在注释登记。
#[allow(dead_code)]
pub fn env_mutation_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// RAII 环境变量守卫：设置于构造、退出时移除（防失败路径污染后续测试）。
/// 持有它时必须同时持有 [`env_mutation_lock`]。
#[allow(dead_code)]
pub struct EnvVarGuard;

#[allow(dead_code)]
impl EnvVarGuard {
    pub fn set(key: &str, val: &str) -> Self {
        unsafe { std::env::set_var(key, val) };
        Self
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        unsafe { std::env::remove_var("RUPOST_HISTORY_DIR") };
    }
}

/// 静音历史落盘：`http_runner` 对每次请求记历史，不静音则测试流量会追加进
/// 开发者真实 `.rupost/history.jsonl`（见 `history::recorder` 模块注释）。
/// 历史功能的专项测试（history_integration_test）自带 env 隔离，不引用本模块。
#[ctor::ctor]
fn isolate_history_recording() {
    rupost::history::recorder::disable_history_recording();
}
