//! 快照落盘动作（R9）：当前请求/响应打包写入 ./snapshots/，默认脱敏。

use crate::tui::state::AppState;

/// R9：保存当前请求/响应快照（默认脱敏）到 ./snapshots/
pub fn save_current_snapshot(state: &mut AppState) {
    use crate::history::model::{RequestSnapshot, ResponseSnapshot, SnapshotEntry, SnapshotSuite};

    let Some(resp) = state.last_response.clone() else {
        state.last_error = Some("no response to snapshot — run a request first".to_string());
        return;
    };
    let Some(req) = state.current_request.clone() else {
        state.last_error = Some("no current request to snapshot".to_string());
        return;
    };

    let mut entry = SnapshotEntry {
        id: uuid::Uuid::new_v4().to_string(),
        request: RequestSnapshot::from_parsed(&req),
        response: ResponseSnapshot {
            status: resp.status.code(),
            headers: resp.headers.clone(),
            body: resp.body.clone(),
        },
    };
    crate::history::storage::mask_snapshot_entry(&mut entry);

    let suite = SnapshotSuite {
        timestamp: chrono::Utc::now(),
        source_file: state.editor_file_path.clone(),
        entries: vec![entry],
    };
    let dir = state.workspace_root.join("snapshots");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        state.last_error = Some(format!("snapshot dir: {e}"));
        return;
    }
    let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let path = dir.join(format!("snapshot_{ts}.json"));
    match crate::history::storage::write_snapshot_suite(&suite, &path) {
        Ok(()) => {
            state.last_error = None;
        }
        Err(e) => state.last_error = Some(format!("snapshot write: {e}")),
    }
}
