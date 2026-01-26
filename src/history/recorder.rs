use crate::history::model::{HistoryEntry, RequestSnapshot, ResponseMeta};
use crate::history::storage::get_storage;
use crate::http::Response;
use chrono::Utc;
use tracing::warn;
use uuid::Uuid;

/// 记录请求历史
///
/// 这是一个 Best-effort 操作，如果写入失败会打印警告，但不会返回错误。
pub fn record_history(request: RequestSnapshot, response: &Response) {
    let history_entry = HistoryEntry {
        id: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        // 使用 Response 中记录的 duration (网络耗时)
        duration_ms: response.duration.as_millis() as u64,
        request,
        response: ResponseMeta {
            status: response.status.code(),
            headers: response.headers.clone(),
        },
    };

    if let Err(e) = get_storage().append(&history_entry) {
        warn!("Failed to save request history: {}", e);
    }
}
