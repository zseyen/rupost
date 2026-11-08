//! 文档域：快速新建用例（输入 → 文件名推导 → 落盘）、树定位与加载、编辑器文件重载。
//!
//! 从 state/mod.rs 外科手术式迁出（解耦提案 v2 Phase 1）；跨文件 `impl AppState`
//! 先例见 stream.rs / tree.rs。

use super::{AppState, Panel};
use crate::tui::files;
use std::path::{Path, PathBuf};

impl AppState {
    pub fn create_test_from_input(&mut self, raw_input: &str) {
        let input = raw_input.trim().to_string();
        if input.is_empty() {
            return;
        }

        let derived = files::derive_test_file(&input);

        // 落盘目录：选中的目录取自身，选中的文件取父目录，无选中回退 workspace_root
        let mut target_dir = self.workspace_root.clone();
        if !self.visible_file_nodes.is_empty()
            && self.files_state.selected_index < self.visible_file_nodes.len()
        {
            let selected_node = &self.visible_file_nodes[self.files_state.selected_index];
            let selected_path = PathBuf::from(&selected_node.abs_path);
            if selected_node.is_dir {
                target_dir = selected_path;
            } else if let Some(parent) = selected_path.parent() {
                target_dir = parent.to_path_buf();
            }
        }

        match files::create_test_in_workspace(
            self.repo.file_repo(),
            &target_dir,
            &self.workspace_root,
            &derived,
        ) {
            Ok(files::CreateOutcome::Created(path)) => {
                self.last_error = None;
                self.locate_created_file(&path, Some(&derived.content));
            }
            Ok(files::CreateOutcome::AlreadyExists(path)) => {
                self.last_error = None;
                self.locate_created_file(&path, None);
            }
            Err(msg) => self.last_error = Some(msg),
        }
    }

    /// 重建树视图并把光标定位到新建/已存在的文件行，加载内容并切焦 Editor
    fn locate_created_file(&mut self, canon_path: &Path, content: Option<&str>) {
        let path_str = canon_path.to_string_lossy().into_owned();
        if !self.raw_file_list.contains(&path_str) {
            self.raw_file_list.push(path_str.clone());
        }
        self.rebuild_visible_tree_nodes();

        if let Some(pos) = self
            .visible_file_nodes
            .iter()
            .position(|n| n.abs_path == path_str)
        {
            self.files_state.selected_index = pos;
            self.selected_file_index = pos;

            // 刚写入的内容直接复用；已存在文件须回读磁盘
            let new_content = match content {
                Some(c) => Some(c.to_string()),
                None => std::fs::read_to_string(canon_path).ok(),
            };
            if let Some(new_content) = new_content {
                self.editor_text = new_content;
                self.editor_file_path = Some(path_str);
                self.editor_file_mtime = std::fs::metadata(canon_path)
                    .ok()
                    .and_then(|m| m.modified().ok());
                self.is_dirty = false;
                self.loaded_file_index = pos;
                self.active_panel = Panel::Editor;
            }
        }
    }

    pub fn check_and_reload_editor_file(&mut self) {
        if let Some(ref path_str) = self.editor_file_path {
            if let Ok(metadata) = std::fs::metadata(path_str) {
                if let Ok(new_mtime) = metadata.modified() {
                    if Some(new_mtime) != self.editor_file_mtime {
                        if let Ok(content) = std::fs::read_to_string(path_str) {
                            self.editor_text = content;
                            self.editor_file_mtime = Some(new_mtime);
                        }
                    }
                }
            }
        }
    }
}
