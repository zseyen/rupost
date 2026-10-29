#![allow(clippy::collapsible_if)]
use super::event::Action;
use super::files;
use super::ports::Repo;
pub use super::theme::ThemeKind;
use super::theme::{Theme, palette};

mod history;
mod layout;
mod retention;
mod stream;
mod tree;
mod wrap;

use crate::assertion::AssertionResult;
use crate::http::Response;
use crate::parser::ParsedRequest;
use crate::variable::VariableContext;
pub use history::{format_request_snapshot_to_http, format_url_host_and_path};
pub use layout::LayoutMode;
pub use retention::{cleanup_old_logs_dir, write_log_with_limit};
use std::collections::HashMap;
pub use tree::{SidebarTabState, TuiFileNode};
pub use wrap::VisualLineProcessor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarTab {
    Files,
    History,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DetailTab {
    #[default]
    Response,
    Headers,
    Request,
}

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// 树状文件夹节点表示，用于可见行的树状渲染
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Files,
    Editor,
    Response,
}

/// 模态输入互斥状态：同一时刻至多一个输入通道打开
#[derive(Debug, Clone)]
pub enum InputMode {
    None,
    /// Box 装箱避免 TextArea（~624B）撑大每次 AppState 克隆
    QuickCreate(Box<ratatui_textarea::TextArea<'static>>),
    /// 侧边栏 `/` 过滤（实时过滤，Esc 清除）
    Filter(Box<ratatui_textarea::TextArea<'static>>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingAction {
    Quit,
    SwitchFile(usize),
    SwitchHistory(usize),
}

#[derive(Debug, Clone)]
pub struct WsFrameRecord {
    pub is_send: bool,
    pub timestamp: String,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileExecState {
    Running,
    Success,
    Failed,
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub active_panel: Panel,
    pub is_loading: bool,
    pub show_help: bool,
    pub layout_mode: LayoutMode,
    pub terminal_width: u16,
    pub terminal_height: u16,

    // 数据模型
    pub visible_file_nodes: Vec<TuiFileNode>, // 展开可见的树节点列表
    pub raw_file_list: Vec<String>,           // 扫描出的原始测试文件物理路径列表
    pub expanded_dirs: HashSet<String>,       // 已展开相对目录路径的集合
    pub workspace_root: PathBuf,              // 工作空间根路径
    pub selected_file_index: usize,
    pub current_request: Option<ParsedRequest>,
    pub last_response: Option<Response>,
    pub assertions: Vec<AssertionResult>,
    /// 最近一次执行的非致命告警（由 RequestFinished 事件写入，SendRequest 时清空）
    pub warnings: Vec<String>,
    pub variables: VariableContext,
    pub is_quitting: bool,

    // 编辑器及未保存确认状态
    pub editor_text: String,
    pub editor_file_path: Option<String>,
    pub editor_file_mtime: Option<std::time::SystemTime>,
    pub is_dirty: bool,
    pub show_unsaved_confirm: bool,
    pub pending_action: Option<PendingAction>,
    pub loaded_file_index: usize,
    pub response_scroll: usize,

    // 长连接缓冲状态
    pub sse_stream_body: String,
    pub ws_frames: Vec<WsFrameRecord>,

    // 瀑布流/滑动视口优化状态
    pub log_file_path: Option<std::path::PathBuf>,
    pub total_log_lines: usize,
    pub viewport_cache: std::collections::VecDeque<WsFrameRecord>,
    pub cache_start_line: usize,
    pub cache_end_line: usize,

    // 新增侧边栏与预折行渲染状态
    pub active_sidebar_tab: SidebarTab,
    pub files_state: SidebarTabState,
    pub history_state: SidebarTabState,
    pub show_full_path: bool,
    pub history_list: Vec<crate::history::model::HistoryEntry>,
    pub response_visual_lines: Vec<ratatui::text::Line<'static>>,
    pub editor_scroll: usize,

    // Phase 3 新增状态与缓存字段
    pub loading_tick: usize,
    pub file_execution_states: std::collections::HashMap<String, FileExecState>,
    pub sse_visual_lines: Vec<ratatui::text::Line<'static>>,
    pub sse_last_processed_pos: usize,
    pub ws_visual_lines: Vec<ratatui::text::Line<'static>>,
    pub current_response_width: usize,

    // Phase 3 过滤状态（空串 = 不过滤）
    pub filter_query: String,

    /// 主题种类（`,` 切换）；取色经 palette()
    pub theme: ThemeKind,

    // Phase 4 新增基础交互字段
    pub active_detail_tab: DetailTab,
    /// 模态输入状态：互斥建模（快速新建 / 未来的过滤输入），textarea 自带编辑与光标
    pub input_mode: InputMode,
    pub quick_input_title: String,
    pub pending_editor_launch: bool,

    // 写路径端口（见 ports.rs）；默认 FsRepository，测试注入 InMemoryRepository
    pub repo: Repo,
    /// 最近一次写路径失败的可读信息，由状态栏渲染；成功路径清除
    pub last_error: Option<String>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            active_panel: Panel::Files,
            is_loading: false,
            show_help: false,
            layout_mode: LayoutMode::Wide,
            terminal_width: 120,
            terminal_height: 30,
            visible_file_nodes: Vec::new(),
            raw_file_list: Vec::new(),
            expanded_dirs: HashSet::new(),
            workspace_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            selected_file_index: 0,
            current_request: None,
            last_response: None,
            assertions: Vec::new(),
            warnings: Vec::new(),
            variables: VariableContext::new(),
            is_quitting: false,
            editor_text: String::new(),
            editor_file_path: None,
            editor_file_mtime: None,
            is_dirty: false,
            show_unsaved_confirm: false,
            pending_action: None,
            loaded_file_index: 0,
            response_scroll: 0,
            sse_stream_body: String::new(),
            ws_frames: Vec::new(),
            log_file_path: None,
            total_log_lines: 0,
            viewport_cache: std::collections::VecDeque::new(),
            cache_start_line: 0,
            cache_end_line: 0,

            // 初始化新字段
            active_sidebar_tab: SidebarTab::Files,
            files_state: SidebarTabState::new(),
            history_state: SidebarTabState::new(),
            show_full_path: false,
            history_list: Vec::new(),
            response_visual_lines: Vec::new(),
            editor_scroll: 0,

            // Phase 3 字段初始化
            loading_tick: 0,
            file_execution_states: std::collections::HashMap::new(),
            sse_visual_lines: Vec::new(),
            sse_last_processed_pos: 0,
            ws_visual_lines: Vec::new(),
            current_response_width: 120, // 默认面板占宽

            // Phase 3 过滤
            filter_query: String::new(),

            theme: ThemeKind::Dark,

            // Phase 4 字段初始化
            active_detail_tab: DetailTab::Response,
            input_mode: InputMode::None,
            quick_input_title: String::new(),
            pending_editor_launch: false,

            repo: Repo::default(),
            last_error: None,
        }
    }

    /// 当前主题调色板
    pub fn palette(&self) -> &'static Theme {
        palette(self.theme)
    }

    /// 注入文件端口（单测用 InMemoryRepository 隔离真实磁盘）
    pub fn with_ports(repo: Repo) -> Self {
        let mut state = Self::new();
        state.repo = repo;
        state
    }

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

    /// 单向状态机更新
    pub fn update(&mut self, action: Action) {
        match action {
            Action::Quit => {
                self.is_quitting = true;
            }
            Action::ToggleHelp => {
                self.show_help = !self.show_help;
            }
            Action::SwitchPanel(panel) => {
                self.active_panel = panel;
            }
            Action::SendRequest(req) => {
                self.current_request = Some(*req);
                self.is_loading = true;
                self.loading_tick = 0;
                self.sse_stream_body.clear();
                self.ws_frames.clear();
                self.warnings.clear();
                self.response_scroll = 0;
                self.response_visual_lines.clear();
                self.sse_visual_lines.clear();
                self.sse_last_processed_pos = 0;
                self.ws_visual_lines.clear();

                if let Some(ref path) = self.editor_file_path {
                    if self.active_sidebar_tab == SidebarTab::Files {
                        self.file_execution_states
                            .insert(path.clone(), FileExecState::Running);
                    }
                }
            }
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
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
}
