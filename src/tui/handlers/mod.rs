//! 交互组件编排：模态 > 确认弹窗 > 全局键 > 面板组件的优先级分发链。
//! 各组件（modal/confirm/sidebar/editor/response/mouse）只依赖 AppState 与 nav 纯函数。

pub mod confirm;
pub mod editor;
pub mod modal;
pub mod mouse;
pub mod nav;
pub mod response;
pub mod sidebar;
pub mod sync;

pub use editor::trigger_request_execution;
pub use mouse::handle_mouse;

use crate::tui::event::{Action, TuiEvent};
use crate::tui::state::{AppState, InputMode, Panel};
use confirm::handle_confirm_key;
use crossterm::event::{KeyCode, KeyEvent};
use editor::handle_editor_key;
use modal::handle_modal_key;
use response::handle_response_key;
use sidebar::handle_sidebar_key;
use tokio::sync::mpsc;

/// 处理 TUI 按键输入事件
pub async fn handle_key(state: &mut AppState, key: KeyEvent, event_tx: &mpsc::Sender<TuiEvent>) {
    // 0. 模态输入状态的前置处理
    if !matches!(state.input_mode, InputMode::None) {
        handle_modal_key(state, key);
        return;
    }

    // 1. 未保存确认弹窗的前置处理
    if state.show_unsaved_confirm {
        handle_confirm_key(state, key);
        return;
    }

    // 全局大写 E 键拦截，用于拉起外部编辑器
    if key.code == KeyCode::Char('E') {
        if state.active_panel != Panel::Response && state.editor_file_path.is_some() {
            state.pending_editor_launch = true;
        }
        return;
    }

    // 全局 Esc 键高优先级拦截（用于关闭其他全局轻量弹窗并返回 Files 面板）
    if key.code == KeyCode::Esc {
        if state.show_help {
            state.show_help = false;
            return;
        }
        if state.active_panel != Panel::Files {
            state.active_panel = Panel::Files;
            return;
        }
    }

    // 2. 全局高优先级指令
    if key.code == KeyCode::Char('q') {
        state.update(Action::Quit);
        return;
    }
    if key.code == KeyCode::Char('?') {
        state.update(Action::ToggleHelp);
        return;
    }
    if key.code == KeyCode::Tab {
        state.update(Action::SwitchPanel(nav::next_panel(
            state.active_panel,
            true,
        )));
        return;
    }
    if key.code == KeyCode::BackTab {
        state.update(Action::SwitchPanel(nav::next_panel(
            state.active_panel,
            false,
        )));
        return;
    }

    // 3. 根据当前激活的面板分发特定的键盘操作
    match state.active_panel {
        Panel::Files => handle_sidebar_key(state, key, event_tx).await,
        Panel::Editor => handle_editor_key(state, key, event_tx).await,
        Panel::Response => handle_response_key(state, key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::state::{PendingAction, SidebarTab};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui_textarea::TextArea;

    #[tokio::test]
    async fn test_handle_global_tab_navigation() {
        let mut state = AppState::new();
        let (tx, _rx) = mpsc::channel(1);

        assert_eq!(state.active_panel, Panel::Files);

        // 模拟 Tab 键
        let tab_key = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
        handle_key(&mut state, tab_key, &tx).await;
        assert_eq!(state.active_panel, Panel::Editor);

        // 再次 Tab 键
        handle_key(&mut state, tab_key, &tx).await;
        assert_eq!(state.active_panel, Panel::Response);

        // 模拟 Shift+Tab (BackTab) 键
        let back_tab_key = KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE);
        handle_key(&mut state, back_tab_key, &tx).await;
        assert_eq!(state.active_panel, Panel::Editor);
    }

    #[tokio::test]
    async fn test_unsaved_confirm_dialog_gate() {
        let mut state = AppState::new();
        let (tx, _rx) = mpsc::channel(1);

        state.show_unsaved_confirm = true;
        state.pending_action = Some(PendingAction::Quit);

        // 模拟按 'N' 取消
        let key_n = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE);
        handle_key(&mut state, key_n, &tx).await;
        assert!(!state.show_unsaved_confirm);
        assert_eq!(state.pending_action, None);
        assert!(!state.is_quitting);

        // 重设为确认
        state.show_unsaved_confirm = true;
        state.pending_action = Some(PendingAction::Quit);
        // 模拟按 'Y' 确认退出
        let key_y = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE);
        handle_key(&mut state, key_y, &tx).await;
        assert!(!state.show_unsaved_confirm);
        assert_eq!(state.pending_action, None);
        assert!(state.is_quitting);
    }

    #[tokio::test]
    async fn test_response_scrolling_limits() {
        let mut state = AppState::new();
        state.active_panel = Panel::Response;
        state.terminal_height = 10; // 可见高度为 10-2 = 8 行

        // 模拟响应折行行数为 12 行
        state.response_visual_lines = vec![ratatui::text::Line::raw("line"); 12];

        let (tx, _rx) = mpsc::channel(1);

        // 初始滚动为 0
        assert_eq!(state.response_scroll, 0);

        // 模拟向下滚动，最大可以滚动到 12 - 8 = 4
        let down_key = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        for _ in 0..10 {
            handle_key(&mut state, down_key, &tx).await;
        }
        assert_eq!(state.response_scroll, 4);

        // 模拟向上滚动
        let up_key = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        handle_key(&mut state, up_key, &tx).await;
        assert_eq!(state.response_scroll, 3);
    }

    #[tokio::test]
    async fn test_modal_gate_blocks_global_keys() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = AppState::new();
        state.workspace_root = dir.path().to_path_buf();
        state.input_mode = crate::tui::state::InputMode::QuickCreate(Box::new(TextArea::default()));
        let (tx, _rx) = mpsc::channel(1);

        // 模态打开时，全局键必须被 textarea 吞掉而非触发全局行为：
        // q 不退出、? 不弹帮助、E 不拉起外部编辑器
        for key in ['q', '?', 'E'] {
            handle_key(
                &mut state,
                KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE),
                &tx,
            )
            .await;
            assert!(!state.is_quitting, "'{key}' must not quit in modal");
            assert!(!state.show_help, "'{key}' must not open help in modal");
            assert!(
                !state.pending_editor_launch,
                "'{key}' must not launch editor in modal"
            );
        }
        assert!(matches!(
            state.input_mode,
            crate::tui::state::InputMode::QuickCreate(_)
        ));
    }

    #[tokio::test]
    async fn test_n_key_opens_quick_input_via_sidebar_dispatch() {
        let mut state = AppState::new();
        state.active_panel = Panel::Files;
        state.active_sidebar_tab = SidebarTab::Files;
        let (tx, _rx) = mpsc::channel(1);

        handle_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
            &tx,
        )
        .await;
        assert!(matches!(
            state.input_mode,
            crate::tui::state::InputMode::QuickCreate(_)
        ));
        assert_eq!(state.quick_input_title, "New HTTP Test File or URL:");
    }
}
