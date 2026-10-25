//! 文件树领域：Trie 构建、可见投影与侧边栏选中态（自 state/mod.rs 拆出）。

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiFileNode {
    pub abs_path: String,
    pub rel_path: String,
    pub display_name: String,
    pub is_dir: bool,
    pub depth: usize,
    pub is_last: bool,
    pub parent_lasts: Vec<bool>,
}

impl TuiFileNode {
    pub fn render_prefix(&self) -> String {
        if self.depth == 0 {
            return String::new(); // 根目录级不画任何连线
        }
        let mut prefix = String::new();
        for (i, &parent_last) in self.parent_lasts.iter().enumerate() {
            if i == 0 {
                prefix.push_str("  "); // 第 0 列始终空白，防悬空
            } else if parent_last {
                prefix.push_str("  ");
            } else {
                prefix.push_str("│ ");
            }
        }
        if self.is_last {
            prefix.push_str("└─ ");
        } else {
            prefix.push_str("├─ ");
        }
        prefix
    }
}

struct TrieNode {
    abs_path: PathBuf,
    rel_path: PathBuf,
    display_name: String,
    is_dir: bool,
    children: BTreeMap<String, TrieNode>,
}

impl TrieNode {
    fn new_root(workspace_root: &Path) -> Self {
        Self {
            abs_path: workspace_root.to_path_buf(),
            rel_path: PathBuf::new(),
            display_name: String::new(),
            is_dir: true,
            children: BTreeMap::new(),
        }
    }

    fn insert(&mut self, rel_path: &Path, _abs_path: &Path) {
        let mut current = self;
        let mut current_rel = PathBuf::new();
        let components: Vec<_> = rel_path.components().collect();
        for (i, component) in components.iter().enumerate() {
            if let std::path::Component::Normal(name_os) = component {
                let name = name_os.to_string_lossy().into_owned();
                current_rel = current_rel.join(&name);
                let step_abs = current.abs_path.join(&name);

                let is_last = i == components.len() - 1;
                let is_component_dir = if !is_last {
                    true
                } else {
                    if step_abs.exists() {
                        step_abs.is_dir()
                    } else {
                        let ext = step_abs
                            .extension()
                            .map(|e| e.to_string_lossy().to_string().to_lowercase());
                        !matches!(ext.as_deref(), Some("http") | Some("md"))
                    }
                };

                current = current
                    .children
                    .entry(name.clone())
                    .or_insert_with(|| TrieNode {
                        abs_path: step_abs,
                        rel_path: current_rel.clone(),
                        display_name: name,
                        is_dir: is_component_dir,
                        children: BTreeMap::new(),
                    });
            }
        }
    }

    fn project_to_visible(
        &self,
        expanded_dirs: &HashSet<String>,
        visible_nodes: &mut Vec<TuiFileNode>,
        depth: usize,
        is_last: bool,
        parent_lasts: Vec<bool>,
    ) {
        let rel_str = self.rel_path.to_string_lossy().replace('\\', "/");
        if !rel_str.is_empty() {
            visible_nodes.push(TuiFileNode {
                abs_path: self.abs_path.to_string_lossy().into_owned(),
                rel_path: rel_str.clone(),
                display_name: self.display_name.clone(),
                is_dir: self.is_dir,
                depth,
                is_last,
                parent_lasts: parent_lasts.clone(),
            });
        }

        if self.is_dir && (rel_str.is_empty() || expanded_dirs.contains(&rel_str)) {
            let child_count = self.children.len();
            for (idx, (_, child)) in self.children.iter().enumerate() {
                let child_is_last = idx == child_count - 1;
                let mut next_parent_lasts = parent_lasts.clone();
                if !rel_str.is_empty() {
                    next_parent_lasts.push(is_last);
                }
                child.project_to_visible(
                    expanded_dirs,
                    visible_nodes,
                    if rel_str.is_empty() { 0 } else { depth + 1 },
                    child_is_last,
                    next_parent_lasts,
                );
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SidebarTabState {
    pub selected_index: usize,
    pub scroll_offset: usize,
}

impl SidebarTabState {
    pub fn new() -> Self {
        Self {
            selected_index: 0,
            scroll_offset: 0,
        }
    }

    pub fn clamp_scroll_offset(&mut self, item_count: usize, viewport_height: usize) {
        if item_count == 0 {
            self.selected_index = 0;
            self.scroll_offset = 0;
            return;
        }
        if self.selected_index >= item_count {
            self.selected_index = item_count - 1;
        }
        // 向上越界
        if self.selected_index < self.scroll_offset {
            self.scroll_offset = self.selected_index;
        }
        // 向下越界
        else if self.selected_index >= self.scroll_offset + viewport_height {
            self.scroll_offset = self.selected_index - viewport_height + 1;
        }
    }
}

impl AppState {
    pub fn toggle_directory(&mut self, index: usize) {
        if index >= self.visible_file_nodes.len() {
            return;
        }
        let rel_path = self.visible_file_nodes[index].rel_path.clone();
        if !self.visible_file_nodes[index].is_dir {
            return;
        }
        if self.expanded_dirs.contains(&rel_path) {
            self.expanded_dirs.remove(&rel_path);
        } else {
            self.expanded_dirs.insert(rel_path);
        }
        self.rebuild_visible_tree_nodes();
    }

    pub fn rebuild_visible_tree_nodes(&mut self) {
        let last_selected_rel_path = if !self.visible_file_nodes.is_empty()
            && self.files_state.selected_index < self.visible_file_nodes.len()
        {
            Some(
                self.visible_file_nodes[self.files_state.selected_index]
                    .rel_path
                    .clone(),
            )
        } else {
            None
        };

        let root_canon = std::fs::canonicalize(&self.workspace_root)
            .unwrap_or_else(|_| self.workspace_root.clone());

        let mut root = TrieNode::new_root(&root_canon);
        let mut unique_paths = HashSet::new();

        for path_str in &self.raw_file_list {
            let path_buf = PathBuf::from(path_str);
            let path_canon = std::fs::canonicalize(&path_buf).unwrap_or(path_buf);

            if let Ok(rel_path) = path_canon.strip_prefix(&root_canon) {
                if unique_paths.insert(rel_path.to_path_buf()) {
                    root.insert(rel_path, &path_canon);
                }
            } else {
                let filename = path_canon
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "unknown.http".to_string());
                let rel = Path::new(&filename);
                if unique_paths.insert(rel.to_path_buf()) {
                    root.insert(rel, &path_canon);
                }
            }
        }

        let mut new_visible_nodes = Vec::new();
        root.project_to_visible(
            &self.expanded_dirs,
            &mut new_visible_nodes,
            0,
            true,
            Vec::new(),
        );
        self.visible_file_nodes = new_visible_nodes;

        // 优雅光标重定位（“向心回弹”算法）
        if let Some(prev_path) = last_selected_rel_path {
            if self.visible_file_nodes.is_empty() {
                self.files_state.selected_index = 0;
            } else {
                let mut found_index = None;
                let mut current_search = PathBuf::from(&prev_path);

                loop {
                    let search_str = current_search.to_string_lossy().replace('\\', "/");
                    if let Some(idx) = self
                        .visible_file_nodes
                        .iter()
                        .position(|n| n.rel_path == search_str)
                    {
                        found_index = Some(idx);
                        break;
                    }
                    if let Some(parent) = current_search.parent() {
                        if parent.as_os_str().is_empty() {
                            break;
                        }
                        current_search = parent.to_path_buf();
                    } else {
                        break;
                    }
                }
                self.files_state.selected_index = found_index
                    .unwrap_or(0)
                    .min(self.visible_file_nodes.len() - 1);
            }
        } else {
            self.files_state.selected_index = self
                .files_state
                .selected_index
                .min(self.visible_file_nodes.len().saturating_sub(1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
