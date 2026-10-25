//! 鼠标组件：坐标命中（纯几何）+ 状态副作用应用，两层解耦。

use super::nav::{self, HitTarget};
use super::sync::sync_preview_to_editor;
use crate::tui::state::{AppState, Panel, SidebarTab};
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

#[cfg(test)]
use crossterm::event::KeyModifiers;

/// 处理鼠标点击：命中测试后应用副作用
pub fn handle_mouse(state: &mut AppState, mouse_event: MouseEvent) {
    if mouse_event.kind != MouseEventKind::Down(MouseButton::Left) {
        return;
    }
    let hit = nav::hit_test(
        state.layout_mode,
        state.terminal_width,
        state.terminal_height,
        mouse_event.column,
        mouse_event.row,
        state.active_panel,
    );
    if let Some(target) = hit {
        apply_hit(state, target);
    }
}

fn apply_hit(state: &mut AppState, target: HitTarget) {
    match target {
        HitTarget::Header { tab } => {
            state.active_panel = Panel::Files;
            if let Some(tab) = tab {
                state.active_sidebar_tab = tab;
            }
            sync_preview_to_editor(state);
        }
        HitTarget::SidebarList(click_row) => {
            state.active_panel = Panel::Files;
            match state.active_sidebar_tab {
                SidebarTab::Files => {
                    let idx = state.files_state.scroll_offset + click_row;
                    if idx < state.visible_file_nodes.len() {
                        if state.visible_file_nodes[idx].is_dir {
                            state.toggle_directory(idx);
                        } else {
                            state.files_state.selected_index = idx;
                            state.selected_file_index = idx;
                            sync_preview_to_editor(state);
                        }
                    }
                }
                SidebarTab::History => {
                    let idx = state.history_state.scroll_offset + click_row;
                    if idx < state.history_list.len() {
                        state.history_state.selected_index = idx;
                        sync_preview_to_editor(state);
                    }
                }
            }
        }
        HitTarget::Panel(panel) => state.active_panel = panel,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mouse_clicking_panel_navigation() {
        let mut state = AppState::new();
        state.terminal_width = 120;
        state.terminal_height = 30;
        state.layout_mode = crate::tui::state::LayoutMode::Wide; // 宽屏布局

        assert_eq!(state.active_panel, Panel::Files);

        // 鼠标点击 Editor 区域 (x = 40, y = 5)
        let click_editor = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 40,
            row: 5,
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse(&mut state, click_editor);
        assert_eq!(state.active_panel, Panel::Editor);

        // 鼠标点击 Response 区域 (x = 90, y = 5)
        let click_response = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 90,
            row: 5,
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse(&mut state, click_response);
        assert_eq!(state.active_panel, Panel::Response);
    }
}
