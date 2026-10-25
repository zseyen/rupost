//! 侧边栏组件（Files / History 双 Tab）的按键处理。

use super::editor::trigger_request_execution;
use super::sync::sync_preview_to_editor;
use crate::tui::event::TuiEvent;
use crate::tui::state::{AppState, InputMode, Panel, SidebarTab};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc;

pub(super) async fn run_selected_sidebar_file(
    state: &mut AppState,
    event_tx: &mpsc::Sender<TuiEvent>,
) {
    if state.active_sidebar_tab == SidebarTab::Files && !state.visible_file_nodes.is_empty() {
        let idx = state.files_state.selected_index;
        if idx < state.visible_file_nodes.len() {
            let node = &state.visible_file_nodes[idx];
            if !node.is_dir
                && let Ok(content) = std::fs::read_to_string(&node.abs_path)
            {
                state.editor_text = content;
                state.editor_file_path = Some(node.abs_path.clone());
                state.editor_file_mtime = std::fs::metadata(&node.abs_path)
                    .map(|m| m.modified().ok())
                    .unwrap_or(None);
                state.is_dirty = false;
                state.loaded_file_index = idx;
                state.selected_file_index = idx;
                state.files_state.selected_index = idx;

                trigger_request_execution(state, event_tx).await;
            }
        }
    }
}

/// 处理侧边栏面板（文件树和历史记录 Tab）的按键
pub(super) async fn handle_sidebar_key(
    state: &mut AppState,
    key: KeyEvent,
    event_tx: &mpsc::Sender<TuiEvent>,
) {
    if state.show_help {
        return;
    }
    match key.code {
        KeyCode::Left | KeyCode::Char('h') => {
            if state.active_sidebar_tab == SidebarTab::Files && !state.visible_file_nodes.is_empty()
            {
                let idx = state.files_state.selected_index;
                if idx < state.visible_file_nodes.len() {
                    let node = &state.visible_file_nodes[idx];
                    if node.is_dir && state.expanded_dirs.contains(&node.rel_path) {
                        state.toggle_directory(idx);
                        return;
                    }
                }
            }
            state.active_sidebar_tab = SidebarTab::Files;
            sync_preview_to_editor(state);
        }
        KeyCode::Right | KeyCode::Char('l') => {
            if state.active_sidebar_tab == SidebarTab::Files && !state.visible_file_nodes.is_empty()
            {
                let idx = state.files_state.selected_index;
                if idx < state.visible_file_nodes.len() {
                    let node = &state.visible_file_nodes[idx];
                    if node.is_dir && !state.expanded_dirs.contains(&node.rel_path) {
                        state.toggle_directory(idx);
                        return;
                    }
                }
            }
            state.active_sidebar_tab = SidebarTab::History;
            sync_preview_to_editor(state);
        }
        KeyCode::Char('p') | KeyCode::Char('P') => {
            if state.active_sidebar_tab == SidebarTab::Files {
                state.show_full_path = !state.show_full_path;
            }
        }
        KeyCode::Char('n') => {
            if state.active_sidebar_tab == SidebarTab::Files {
                let mut textarea = ratatui_textarea::TextArea::default();
                textarea
                    .set_style(ratatui::style::Style::default().fg(ratatui::style::Color::White));
                state.input_mode = InputMode::QuickCreate(Box::new(textarea));
                state.quick_input_title = "New HTTP Test File or URL:".to_string();
            }
        }
        KeyCode::Char('r') | KeyCode::Char('R') | KeyCode::Enter
            if key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            run_selected_sidebar_file(state, event_tx).await;
        }
        KeyCode::Char('r') | KeyCode::Char('R') => {
            run_selected_sidebar_file(state, event_tx).await;
        }
        KeyCode::Down | KeyCode::Char('j') => match state.active_sidebar_tab {
            SidebarTab::Files => {
                if state.files_state.selected_index + 1 < state.visible_file_nodes.len() {
                    state.files_state.selected_index += 1;
                    state.selected_file_index = state.files_state.selected_index;
                    sync_preview_to_editor(state);
                }
            }
            SidebarTab::History => {
                if state.history_state.selected_index + 1 < state.history_list.len() {
                    state.history_state.selected_index += 1;
                    sync_preview_to_editor(state);
                }
            }
        },
        KeyCode::Up | KeyCode::Char('k') => match state.active_sidebar_tab {
            SidebarTab::Files => {
                if state.files_state.selected_index > 0 {
                    state.files_state.selected_index -= 1;
                    state.selected_file_index = state.files_state.selected_index;
                    sync_preview_to_editor(state);
                }
            }
            SidebarTab::History => {
                if state.history_state.selected_index > 0 {
                    state.history_state.selected_index -= 1;
                    sync_preview_to_editor(state);
                }
            }
        },
        KeyCode::Enter => match state.active_sidebar_tab {
            SidebarTab::Files => {
                if !state.visible_file_nodes.is_empty() {
                    let idx = state.files_state.selected_index;
                    if idx < state.visible_file_nodes.len() {
                        let node = &state.visible_file_nodes[idx];
                        if node.is_dir {
                            state.toggle_directory(idx);
                        } else {
                            state.active_panel = Panel::Editor;
                        }
                    }
                }
            }
            SidebarTab::History => {
                if !state.history_list.is_empty() {
                    state.active_panel = Panel::Editor;
                }
            }
        },
        _ => {}
    }
}
