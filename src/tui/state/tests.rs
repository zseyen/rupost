//! state 域单测（自 mod.rs 迁出，解耦提案 v2 Phase 1）：
//! 侧栏滚动钳制与快速新建（quick create）全场景守护。

use super::*;
use crate::tui::ports::Repo;
use std::path::Path;

#[test]
fn test_sidebar_scroll_bounds_clamp() {
    let mut state = SidebarTabState::new();
    let viewport_height = 5;

    state.clamp_scroll_offset(0, viewport_height);
    assert_eq!(state.selected_index, 0);
    assert_eq!(state.scroll_offset, 0);

    state.selected_index = 2;
    state.clamp_scroll_offset(10, viewport_height);
    assert_eq!(state.scroll_offset, 0);

    state.selected_index = 6;
    state.clamp_scroll_offset(10, viewport_height);
    assert_eq!(state.scroll_offset, 2);

    state.selected_index = 1;
    state.clamp_scroll_offset(10, viewport_height);
    assert_eq!(state.scroll_offset, 1);
}
fn state_in_workspace(root: &Path) -> AppState {
    let mut state = AppState::new();
    state.workspace_root = root.to_path_buf();
    state
}

#[test]
fn test_quick_create_success_lock_behavior() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = state_in_workspace(dir.path());
    state.create_test_from_input("google.com");

    assert_eq!(state.last_error, None);
    let created = dir.path().join("google_com.http");
    assert!(created.exists());
    assert_eq!(
        std::fs::read_to_string(&created).unwrap(),
        "GET https://google.com\n"
    );
    assert!(
        state
            .editor_file_path
            .as_deref()
            .unwrap()
            .ends_with("google_com.http")
    );
    assert_eq!(state.active_panel, Panel::Editor);
}

#[test]
fn test_quick_create_path_traversal_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = state_in_workspace(dir.path());
    // 唯一文件名保证测试密闭：工作区外绝无同名残留（红阶段曾实证旧代码会写出该文件）
    let escape_name = format!("escape_{}.http", std::process::id());
    state.create_test_from_input(&format!("../../{escape_name}"));

    assert!(
        state.last_error.is_some(),
        "traversal must surface an error"
    );
    // 显式拼出逃逸落点（相对 tempdir 两级之上）：文件绝不能被写出工作区
    let escape_target = dir.path().join(format!("../../{escape_name}"));
    assert!(
        !escape_target.exists(),
        "escape file must not exist: {escape_target:?}"
    );
}

#[test]
fn test_quick_create_write_error_surfaced() {
    // FailingRepository 注入（替代 chmod：root 下 0o555 不拦截写，权限测试会假失败）
    let dir = tempfile::tempdir().unwrap();
    let mut state = AppState::with_ports(Repo::new(crate::tui::ports::FailingRepository));
    state.workspace_root = dir.path().to_path_buf();
    state.create_test_from_input("google.com");

    assert!(
        state.last_error.is_some(),
        "write failure must surface, not be swallowed"
    );

    // 错误在下次成功操作时清除（不变量锁定）
    state.repo = Repo::default();
    state.create_test_from_input("example.org");
    assert_eq!(state.last_error, None);
    assert!(dir.path().join("example_org.http").exists());
}

#[test]
fn test_quick_create_empty_input_is_noop() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = state_in_workspace(dir.path());
    state.create_test_from_input("   ");

    assert_eq!(state.last_error, None);
    assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
    assert!(state.editor_file_path.is_none());
}

#[test]
fn test_quick_create_lands_in_selected_files_parent_dir() {
    // 锁定改动前行为：选中文件 → 新文件落在其父目录
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("tests");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(sub.join("auth.http"), "GET http://x").unwrap();

    let mut state = state_in_workspace(dir.path());
    state.raw_file_list = vec![sub.join("auth.http").to_string_lossy().into_owned()];
    state.expanded_dirs.insert("tests".to_string());
    state.rebuild_visible_tree_nodes();
    let pos = state
        .visible_file_nodes
        .iter()
        .position(|n| n.rel_path == "tests/auth.http")
        .unwrap();
    state.files_state.selected_index = pos;

    state.create_test_from_input("google.com");
    assert_eq!(state.last_error, None);
    assert!(sub.join("google_com.http").exists());
    assert!(!dir.path().join("google_com.http").exists());
}

#[test]
fn test_quick_create_existing_file_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("google_com.http");
    std::fs::write(&target, "SENTINEL").unwrap();

    let mut state = state_in_workspace(dir.path());
    state.create_test_from_input("google.com");

    assert_eq!(std::fs::read_to_string(&target).unwrap(), "SENTINEL");
    assert_eq!(state.editor_text, "SENTINEL"); // 仍定位并加载已有文件
}

#[test]
fn test_quick_create_filename_truncated() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = state_in_workspace(dir.path());
    let long_path = "a".repeat(300);
    state.create_test_from_input(&format!("https://example.com/{long_path}"));

    assert_eq!(state.last_error, None);
    let entries: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
    assert_eq!(entries.len(), 1);
    let name = entries[0]
        .as_ref()
        .unwrap()
        .file_name()
        .into_string()
        .unwrap();
    // base(≤80) + '_' + 8 hex + ".http"
    assert!(
        name.len() <= 80 + 1 + 8 + 5,
        "sanitized name must be bounded: {name}"
    );
}
