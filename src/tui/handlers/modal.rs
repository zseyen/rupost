//! 模态输入组件：快速新建用例输入条（textarea 接管编辑与光标）。

use crate::tui::state::{AppState, InputMode};
use crossterm::event::{KeyCode, KeyEvent};

pub(super) fn handle_modal_key(state: &mut AppState, key: KeyEvent) {
    match (&state.input_mode, key.code) {
        (_, KeyCode::Esc) => {
            if matches!(state.input_mode, InputMode::Filter(_)) {
                state.set_filter(""); // Esc 同时清除过滤
            }
            state.input_mode = InputMode::None;
        }
        (InputMode::QuickCreate(_), KeyCode::Enter) => {
            if let InputMode::QuickCreate(textarea) = &state.input_mode {
                let value = textarea.lines().concat();
                state.input_mode = InputMode::None;
                state.create_test_from_input(&value);
            }
        }
        (InputMode::Filter(_), KeyCode::Enter) => {
            state.input_mode = InputMode::None; // 过滤已实时生效，Enter 仅关闭
        }
        (InputMode::QuickCreate(_) | InputMode::Filter(_), _) => {
            let is_filter = matches!(state.input_mode, InputMode::Filter(_));
            // 取出 textarea 编辑后放回，避免整段克隆
            let mut textarea = match &mut state.input_mode {
                InputMode::QuickCreate(t) | InputMode::Filter(t) => std::mem::take(t),
                InputMode::None => return,
            };
            textarea.input(crossterm::event::Event::Key(key));
            let query = textarea.lines().concat();
            state.input_mode = if is_filter {
                InputMode::Filter(textarea)
            } else {
                InputMode::QuickCreate(textarea)
            };
            if is_filter {
                state.set_filter(&query); // 实时过滤
            }
        }
        (InputMode::None, _) => {}
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui_textarea::TextArea;
    use tempfile::tempdir;

    #[test]
    fn test_filter_flow_via_key_events() {
        let dir = tempdir().unwrap();
        let mut state = AppState::new();
        state.workspace_root = dir.path().to_path_buf();
        let sub = dir.path().join("tests");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("auth.http"), "GET http://x").unwrap();
        std::fs::write(dir.path().join("readme.http"), "GET http://y").unwrap();
        state.raw_file_list = vec![
            sub.join("auth.http").to_string_lossy().into_owned(),
            dir.path()
                .join("readme.http")
                .to_string_lossy()
                .into_owned(),
        ];
        state.rebuild_visible_tree_nodes();
        state.input_mode = InputMode::Filter(Box::new(TextArea::default()));

        // 逐键输入 'a' 'u' —— 实时过滤生效
        for c in "au".chars() {
            handle_modal_key(
                &mut state,
                KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
            );
        }
        let rels: Vec<String> = state
            .visible_file_nodes
            .iter()
            .map(|n| n.rel_path.clone())
            .collect();
        assert!(
            !state.filter_query.is_empty(),
            "live filter applied: {rels:?}"
        );

        // Esc 清除过滤并关闭模态
        handle_modal_key(&mut state, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(matches!(state.input_mode, InputMode::None));
        assert_eq!(state.filter_query, "");
        assert!(!state.visible_file_nodes.is_empty());
    }

    #[test]
    fn test_quick_create_flow_via_key_events() {
        let dir = tempdir().unwrap();
        let mut state = AppState::new();
        state.workspace_root = dir.path().to_path_buf();
        state.input_mode = InputMode::QuickCreate(Box::new(TextArea::default()));
        assert!(matches!(state.input_mode, InputMode::QuickCreate(_)));

        // 逐键输入（多字节字符交给 textarea 处理，此处验证 ASCII 主路径）
        for c in "google.com".chars() {
            handle_modal_key(
                &mut state,
                KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
            );
        }
        handle_modal_key(
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

        handle_modal_key(
            &mut state,
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE),
        );
        handle_modal_key(&mut state, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(matches!(state.input_mode, InputMode::None));
        // Esc 丢弃：什么都不创建
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
    }
}
