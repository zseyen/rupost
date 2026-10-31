//! 交互副作用同步：侧边栏选中项到编辑器/响应面板的联动加载。

use crate::tui::state::{AppState, SidebarTab};
// ==========================================
// 辅助状态同步函数 (交互内部使用)
// ==========================================

pub(super) fn load_history_response_to_state(state: &mut AppState) {
    if !state.history_list.is_empty()
        && state.history_state.selected_index < state.history_list.len()
    {
        let entry = &state.history_list[state.history_state.selected_index];
        if let Ok(resp) = crate::http::Response::new(
            entry.response.status,
            entry.response.headers.clone(),
            entry
                .response
                .body
                .clone()
                .unwrap_or_else(|| "Body not recorded".to_string()),
            std::time::Duration::from_millis(entry.duration_ms),
            std::time::Duration::ZERO,
            std::time::Duration::ZERO,
        ) {
            state.last_response = Some(resp);
            state.response_visual_lines.clear();
            state.response_scroll = 0;
        }
    }
}

pub(super) fn sync_preview_to_editor(state: &mut AppState) {
    if state.is_dirty {
        return;
    }
    match state.active_sidebar_tab {
        SidebarTab::Files => {
            if !state.visible_file_nodes.is_empty()
                && state.files_state.selected_index < state.visible_file_nodes.len()
            {
                let node = &state.visible_file_nodes[state.files_state.selected_index];
                if !node.is_dir
                    && let Ok(content) = std::fs::read_to_string(&node.abs_path)
                {
                    state.editor_text = content;
                    state.editor_file_path = Some(node.abs_path.clone());
                    if let Ok(metadata) = std::fs::metadata(&node.abs_path) {
                        state.editor_file_mtime = metadata.modified().ok();
                    } else {
                        state.editor_file_mtime = None;
                    }
                    state.is_dirty = false;
                    state.loaded_file_index = state.files_state.selected_index;
                    state.editor_scroll = 0;
                }
            }
        }
        SidebarTab::History => {
            if !state.history_list.is_empty()
                && state.history_state.selected_index < state.history_list.len()
            {
                let entry = &state.history_list[state.history_state.selected_index];
                let http_text = crate::tui::state::format_request_snapshot_to_http(&entry.request);
                state.editor_text = http_text;
                state.editor_file_path = None;
                state.editor_file_mtime = None;
                state.is_dirty = false;
                load_history_response_to_state(state);
                state.editor_scroll = 0;
            }
        }
    }
}
