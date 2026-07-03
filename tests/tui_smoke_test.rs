use rupost::http::Response;
use rupost::parser::ParsedRequest;
use rupost::tui::event::Action;
use rupost::tui::state::{AppState, LayoutMode, Panel};
use std::collections::HashMap;

#[test]
fn test_app_state_initialization() {
    let state = AppState::new();
    assert_eq!(state.active_panel, Panel::Files);
    assert!(!state.is_loading);
    assert!(!state.show_help);
    assert!(!state.is_quitting);
    assert_eq!(state.selected_file_index, 0);
}

#[test]
fn test_adaptive_layout_calculations() {
    let mut state = AppState::new();

    // 1. 宽屏模式测试
    state.update_layout(130, 40);
    assert_eq!(state.layout_mode, LayoutMode::Wide);

    // 2. 窄屏模式测试
    state.update_layout(95, 30);
    assert_eq!(state.layout_mode, LayoutMode::Narrow);

    // 3. 超窄屏堆叠模式测试
    state.update_layout(60, 20);
    assert_eq!(state.layout_mode, LayoutMode::Stacked);

    // 4. 防 Panic 尺寸硬限制下限测试 (如 w=30, h=5)
    state.update_layout(30, 5);
    assert_eq!(state.layout_mode, LayoutMode::Stacked);
    assert_eq!(state.terminal_width, 30);
    assert_eq!(state.terminal_height, 5);
}

#[test]
fn test_state_transitions_via_actions() {
    let mut state = AppState::new();

    // 测试 ToggleHelp
    state.update(Action::ToggleHelp);
    assert!(state.show_help);
    state.update(Action::ToggleHelp);
    assert!(!state.show_help);

    // 测试 SwitchPanel
    state.update(Action::SwitchPanel(Panel::Editor));
    assert_eq!(state.active_panel, Panel::Editor);

    // 测试 SendRequest 会触发 Loading
    let req = ParsedRequest::new(1);
    state.update(Action::SendRequest(Box::new(req)));
    assert!(state.is_loading);
    assert!(state.current_request.is_some());

    // 测试 Quit
    state.update(Action::Quit);
    assert!(state.is_quitting);
}

#[test]
fn test_request_finished_variable_extension() {
    let mut state = AppState::new();
    state.is_loading = true;

    let mut captured_vars = HashMap::new();
    captured_vars.insert("session_token".to_string(), "abc-123-xyz".to_string());
    captured_vars.insert("user_id".to_string(), "42".to_string());

    let response = Response::new(
        200,
        reqwest::header::HeaderMap::new(),
        "{\"status\":\"ok\"}".to_string(),
        std::time::Duration::from_millis(150),
        std::time::Duration::from_millis(50),
        std::time::Duration::from_millis(100),
    )
    .unwrap();

    // 模拟后台返回 RequestFinished 消息
    state.handle_request_finished(Ok(response), captured_vars, Vec::new());

    // 验证 Loading 结束且状态正确合并
    assert!(!state.is_loading);
    assert!(state.last_response.is_some());
    assert_eq!(state.variables.get("session_token").unwrap(), "abc-123-xyz");
    assert_eq!(state.variables.get("user_id").unwrap(), "42");
}

#[test]
fn test_unsaved_changes_confirm_modal() {
    use rupost::tui::state::PendingAction;
    let mut state = AppState::new();

    // 默认弹窗是关闭的
    assert!(!state.show_unsaved_confirm);
    assert_eq!(state.pending_action, None);

    // 模拟编辑器变脏
    state.is_dirty = true;
    state.loaded_file_index = 0;
    state.selected_file_index = 1;

    // 模拟在变脏时触发退出拦截
    state.show_unsaved_confirm = true;
    state.pending_action = Some(PendingAction::Quit);

    // 模拟取消
    state.selected_file_index = state.loaded_file_index;
    state.show_unsaved_confirm = false;
    state.pending_action = None;

    assert_eq!(state.selected_file_index, 0);
    assert!(!state.show_unsaved_confirm);

    // 模拟确认放弃修改
    state.pending_action = Some(PendingAction::Quit);
    state.is_quitting = true;

    assert!(state.is_quitting);
}

#[test]
fn test_response_scroll_initialization_and_mutation() {
    let mut state = AppState::new();
    assert_eq!(state.response_scroll, 0);

    state.response_scroll = state.response_scroll.saturating_add(1);
    assert_eq!(state.response_scroll, 1);

    state.response_scroll = state.response_scroll.saturating_sub(1);
    assert_eq!(state.response_scroll, 0);
}
