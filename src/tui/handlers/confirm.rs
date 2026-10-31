//! 未保存改动强确认弹窗组件。

use super::sync::load_history_response_to_state;
use crate::tui::state::{AppState, Panel, PendingAction};
use crossterm::event::{KeyCode, KeyEvent};
/// 处理未保存改动强确认弹窗的按键
pub(super) fn handle_confirm_key(state: &mut AppState, key: KeyEvent) {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            if let Some(action) = state.pending_action {
                match action {
                    PendingAction::Quit => {
                        state.is_quitting = true;
                    }
                    PendingAction::SwitchFile(idx) => {
                        if idx < state.visible_file_nodes.len() {
                            let node = &state.visible_file_nodes[idx];
                            if !node.is_dir
                                && let Ok(content) = std::fs::read_to_string(&node.abs_path)
                            {
                                state.editor_text = content;
                                state.editor_file_path = Some(node.abs_path.clone());
                                state.is_dirty = false;
                                state.loaded_file_index = idx;
                                state.selected_file_index = idx;
                                state.files_state.selected_index = idx;
                                state.editor_scroll = 0;
                                state.active_panel = Panel::Editor;
                            }
                        }
                    }
                    PendingAction::SwitchHistory(idx) => {
                        if idx < state.history_list.len() {
                            state.history_state.selected_index = idx;
                            let entry = &state.history_list[idx];
                            let http_text =
                                crate::tui::state::format_request_snapshot_to_http(&entry.request);
                            state.editor_text = http_text;
                            state.editor_file_path = None;
                            state.is_dirty = false;
                            load_history_response_to_state(state);
                            state.editor_scroll = 0;
                            state.active_panel = Panel::Editor;
                        }
                    }
                }
            }
            state.show_unsaved_confirm = false;
            state.pending_action = None;
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            state.selected_file_index = state.loaded_file_index;
            state.files_state.selected_index = state.loaded_file_index;
            state.show_unsaved_confirm = false;
            state.pending_action = None;
        }
        _ => {}
    }
}
