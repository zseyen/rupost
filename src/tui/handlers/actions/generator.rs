//! 历史生成动作（R9）：History 侧栏 `G` 一键把历史条目反向生成 .http 写入工作区。
//! 注意：调用方是 sidebar.rs（History Tab），不在 editor 按键分发内。

use crate::tui::state::AppState;

/// R9：History 侧栏 `G` 一键生成 .http（复用 HttpGenerator 语义化命名）写入工作区
pub async fn generate_history_to_file(state: &mut AppState) {
    use crate::generator::HttpGenerator;
    let idx = state.history_state.selected_index;
    let Some(entry) = state.history_list.get(idx).cloned() else {
        state.last_error = Some("no history entry selected".to_string());
        return;
    };
    match HttpGenerator::generate_with_variables(std::slice::from_ref(&entry), false) {
        Ok((content, warnings)) => {
            let id8: String = entry.id.chars().take(8).collect();
            let path = state.workspace_root.join(format!("generated_{id8}.http"));
            match std::fs::write(&path, &content) {
                Ok(()) => {
                    let path_str = path.to_string_lossy().into_owned();
                    if !state.raw_file_list.contains(&path_str) {
                        state.raw_file_list.push(path_str.clone());
                    }
                    state.rebuild_visible_tree_nodes();
                    if let Some(pos) = state
                        .visible_file_nodes
                        .iter()
                        .position(|n| n.abs_path == path_str)
                    {
                        state.files_state.selected_index = pos;
                        state.editor_text = content;
                        state.editor_file_path = Some(path_str);
                        state.active_panel = crate::tui::state::Panel::Editor;
                    }
                    if warnings.is_empty() {
                        state.last_error = None;
                    } else {
                        state.last_error =
                            Some(format!("generated with {} warning(s)", warnings.len()));
                    }
                }
                Err(e) => state.last_error = Some(format!("generate write: {e}")),
            }
        }
        Err(e) => state.last_error = Some(format!("generate: {e}")),
    }
}
