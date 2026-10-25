//! 模态输入组件：快速新建用例输入条（textarea 接管编辑与光标）。

use crate::tui::state::{AppState, InputMode};
use crossterm::event::{KeyCode, KeyEvent};

pub(super) fn handle_quick_input_key(state: &mut AppState, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => state.input_mode = InputMode::None,
        KeyCode::Enter => {
            if let InputMode::QuickCreate(textarea) = &state.input_mode {
                let value = textarea.lines().concat();
                state.input_mode = InputMode::None;
                state.create_test_from_input(&value);
            }
        }
        // 其余按键（含 Backspace/方向键/多字节字符）交给 textarea 处理
        _ => {
            if let InputMode::QuickCreate(textarea) = &mut state.input_mode {
                textarea.input(crossterm::event::Event::Key(key));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui_textarea::TextArea;
    use tempfile::tempdir;

    #[test]
    fn test_quick_create_flow_via_key_events() {
        let dir = tempdir().unwrap();
        let mut state = AppState::new();
        state.workspace_root = dir.path().to_path_buf();
        state.input_mode = InputMode::QuickCreate(Box::new(TextArea::default()));
        assert!(matches!(state.input_mode, InputMode::QuickCreate(_)));

        // 逐键输入（多字节字符交给 textarea 处理，此处验证 ASCII 主路径）
        for c in "google.com".chars() {
            handle_quick_input_key(
                &mut state,
                KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
            );
        }
        handle_quick_input_key(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );

        assert!(matches!(state.input_mode, InputMode::None));
        assert_eq!(state.last_error, None);
        assert!(dir.path().join("google_com.http").exists());
        assert!(
            state
                .editor_file_path
                .as_deref()
                .unwrap()
                .ends_with("google_com.http")
        );
    }

    #[test]
    fn test_quick_create_esc_discards() {
        let dir = tempdir().unwrap();
        let mut state = AppState::new();
        state.workspace_root = dir.path().to_path_buf();
        state.input_mode = InputMode::QuickCreate(Box::new(TextArea::default()));

        handle_quick_input_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
        );
        handle_quick_input_key(&mut state, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(matches!(state.input_mode, InputMode::None));
        // Esc 丢弃：什么都不创建
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
    }
}
