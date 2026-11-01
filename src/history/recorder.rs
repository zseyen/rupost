use crate::history::model::{HistoryEntry, RequestSnapshot, ResponseMeta};
use crate::history::storage::get_storage;
use crate::http::Response;
use chrono::Utc;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::warn;
use uuid::Uuid;

/// 进程级历史落盘开关（默认开）。
///
/// `http_runner` 对每次请求都记历史，若测试进程不静音，测试流量会追加进开发者
/// 真实 `.rupost/history.jsonl`（写入不可撤销；单例目录在首次落盘时固化，事后
/// 重定向 env 无效）。静音策略：
/// - lib 单测进程：编译期 `cfg!(test)` 自动关（无需逐测试处理）；
/// - 集成测试进程：`tests/common` 的 `#[ctor]` pre-main 调用
///   [`disable_history_recording`]（测试线程并行启动，逐测试设置存在竞态）。
static RECORDING_ENABLED: AtomicBool = AtomicBool::new(!cfg!(test));

/// 进程级禁用历史落盘（幂等，进程内不可逆）。仅供测试进程初始化使用。
pub fn disable_history_recording() {
    RECORDING_ENABLED.store(false, Ordering::SeqCst);
}

/// 记录请求历史
///
/// 这是一个 Best-effort 操作，如果写入失败会打印警告，但不会返回错误。
pub fn record_history(request: RequestSnapshot, response: &Response, source: Option<String>) {
    if !RECORDING_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let history_entry = HistoryEntry {
        id: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        // 使用 Response 中记录的 duration (网络耗时)
        duration_ms: response.duration.as_millis() as u64,
        request,
        source,
        response: ResponseMeta {
            status: response.status.code(),
            headers: response.headers.clone(),
            body: Some(response.body.clone()),
        },
    };

    if let Err(e) = get_storage().append(&history_entry) {
        warn!("Failed to save request history: {}", e);
    }
}
