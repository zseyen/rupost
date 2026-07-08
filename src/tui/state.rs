#![allow(clippy::collapsible_if)]
use super::event::Action;
use crate::assertion::AssertionResult;
use crate::http::Response;
use crate::parser::ParsedRequest;
use crate::variable::VariableContext;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarTab {
    Files,
    History,
}

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

/// 树状文件夹节点表示，用于可见行的树状渲染
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Files,
    Editor,
    Response,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    Wide,
    Narrow,
    Stacked,
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
    pub variables: VariableContext,
    pub quick_input: Option<String>,
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
            variables: VariableContext::new(),
            quick_input: None,
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
        }
    }

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

    /// 防御性自适应布局尺寸计算
    pub fn update_layout(&mut self, width: u16, height: u16) {
        self.terminal_width = width;
        self.terminal_height = height;

        // 硬下限防御：如果极小，强制为 Stacked，UI 会渲染简易警告
        if width < 40 || height < 10 {
            self.layout_mode = LayoutMode::Stacked;
            return;
        }

        if width >= 120 {
            self.layout_mode = LayoutMode::Wide;
        } else if width >= 80 {
            self.layout_mode = LayoutMode::Narrow;
        } else {
            self.layout_mode = LayoutMode::Stacked;
        }

        let response_width = match self.layout_mode {
            LayoutMode::Wide => {
                let w = (width as u32 * 35 / 100) as u16;
                w.saturating_sub(2) as usize
            }
            LayoutMode::Narrow => {
                let w = (width as u32 * 40 / 100) as u16;
                w.saturating_sub(2) as usize
            }
            LayoutMode::Stacked => width.saturating_sub(2) as usize,
        };

        if response_width != self.current_response_width {
            self.current_response_width = response_width;
            self.response_visual_lines.clear();
            self.sse_visual_lines.clear();
            self.sse_last_processed_pos = 0;
            self.ws_visual_lines.clear();
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
            Action::UpdateQuickInput(val) => {
                self.quick_input = Some(val);
            }
        }
    }

    pub fn get_spinner_char(&self) -> &'static str {
        let spinner_chars = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        spinner_chars[(self.loading_tick / 2) % spinner_chars.len()]
    }

    pub fn handle_stream_chunk(&mut self, chunk: String) {
        self.sse_stream_body.push_str(&chunk);
        self.total_log_lines += chunk.matches('\n').count();

        let max_width = self.current_response_width;
        let body_ref = &self.sse_stream_body;

        if let Some(last_newline_idx) = body_ref[self.sse_last_processed_pos..].rfind('\n') {
            let actual_newline_idx = self.sse_last_processed_pos + last_newline_idx;
            let completed_text = &body_ref[self.sse_last_processed_pos..=actual_newline_idx];
            for line in completed_text.lines() {
                let wrapped = VisualLineProcessor::wrap_text(line, max_width);
                if wrapped.is_empty() {
                    self.sse_visual_lines.push(ratatui::text::Line::from(""));
                } else {
                    self.sse_visual_lines.extend(wrapped);
                }
            }
            self.sse_last_processed_pos = actual_newline_idx + 1;
        }

        if self.active_panel == Panel::Response {
            self.response_scroll = 9999;
        }
    }

    pub fn handle_ws_frame(&mut self, is_send: bool, content: String) {
        self.total_log_lines += 1;
        let now = chrono::Local::now().format("%H:%M:%S").to_string();
        let arrow = if is_send { "->" } else { "<-" };
        let text_line = format!("{} {} {}", arrow, now, content);
        let max_width = self.current_response_width;
        let wrapped = VisualLineProcessor::wrap_text(&text_line, max_width);

        self.ws_frames.push(WsFrameRecord {
            is_send,
            timestamp: now,
            content,
        });

        if self.ws_frames.len() > 100 {
            self.ws_frames.remove(0);
            self.ws_visual_lines.clear();
            for f in &self.ws_frames {
                let a = if f.is_send { "->" } else { "<-" };
                let t = format!("{} {} {}", a, f.timestamp, f.content);
                let w = VisualLineProcessor::wrap_text(&t, max_width);
                self.ws_visual_lines.extend(w);
            }
        } else {
            self.ws_visual_lines.extend(wrapped);
        }

        if self.active_panel == Panel::Response {
            self.response_scroll = 9999;
        }
    }

    pub fn handle_request_finished(
        &mut self,
        result: Result<Response, String>,
        captured_vars: HashMap<String, String>,
        assertions: Vec<AssertionResult>,
    ) {
        self.is_loading = false;
        self.assertions = assertions.clone();

        let is_success = result.is_ok() && assertions.iter().all(|a| a.passed);
        let exec_state = if is_success {
            FileExecState::Success
        } else {
            FileExecState::Failed
        };

        if let Some(ref path) = self.editor_file_path {
            if self.active_sidebar_tab == SidebarTab::Files {
                self.file_execution_states.insert(path.clone(), exec_state);
            }
        }

        match result {
            Ok(resp) => {
                self.last_response = Some(resp);
                self.variables.extend(captured_vars);
            }
            Err(e) => {
                self.last_response = Some(Response::error(e));
            }
        }
    }
    pub fn load_viewport_sliding_window(&mut self, scroll_y: usize, height: usize) {
        let file_path = match &self.log_file_path {
            Some(p) => p,
            None => return,
        };

        let buffer_size = 50; // 前后缓冲区扩展行数
        let start = scroll_y.saturating_sub(buffer_size);
        let end = (scroll_y + height + buffer_size).min(self.total_log_lines);

        // 如果当前的缓存已经包含了我们计算出来的范围，并且不为空，则不需要重新读取物理文件
        if !self.viewport_cache.is_empty()
            && start >= self.cache_start_line
            && end <= self.cache_end_line
        {
            return;
        }

        // 重新读取文件指定行
        if let Ok(file) = std::fs::File::open(file_path) {
            use std::io::BufRead;
            let reader = std::io::BufReader::new(file);
            let mut new_cache = std::collections::VecDeque::new();

            for (idx, line) in reader.lines().enumerate() {
                if idx < start {
                    continue;
                }
                if idx >= end {
                    break;
                }
                if let Ok(l) = line {
                    let is_send = l.starts_with("[→]");
                    let is_recv = l.starts_with("[←]");
                    let (timestamp, content) = if (is_send || is_recv) && l.len() >= 15 {
                        let ts = l.chars().skip(4).take(8).collect::<String>();
                        let c = l.chars().skip(15).collect::<String>();
                        (ts, c)
                    } else {
                        (
                            chrono::Local::now().format("%H:%M:%S").to_string(),
                            l.clone(),
                        )
                    };

                    new_cache.push_back(WsFrameRecord {
                        is_send,
                        timestamp,
                        content,
                    });
                }
            }

            self.viewport_cache = new_cache;
            self.cache_start_line = start;
            self.cache_end_line = end;
        }
    }
}

/// 通用限额落盘函数：如果日志大小超出限制，则停止追加并写入警告
pub fn write_log_with_limit(
    file_path: &std::path::Path,
    data: &str,
    max_bytes: usize,
) -> Result<(), std::io::Error> {
    use std::fs::OpenOptions;
    use std::io::Write;

    if let Some(parent) = file_path.parent() {
        if !parent.exists() {
            let _ = std::fs::create_dir_all(parent);
        }
    }

    let file_len = file_path.metadata().map(|m| m.len() as usize).unwrap_or(0);
    if file_len >= max_bytes {
        return Ok(());
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(file_path)?;

    let new_len = file_len + data.len();
    if new_len > max_bytes {
        file.write_all(b"\n[SYSTEM] Log truncated due to size limit\n")?;
    } else {
        file.write_all(data.as_bytes())?;
    }
    file.flush()?;
    Ok(())
}

/// 自动清理 7 天前过期的日志目录文件
pub fn cleanup_old_logs_dir(dir_path: &std::path::Path, days: u64) -> Result<(), std::io::Error> {
    crate::runner::gc::perform_prune(dir_path, days, 999999).map(|_| ())
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

pub struct VisualLineProcessor;

impl VisualLineProcessor {
    pub fn wrap_text(text: &str, max_width: usize) -> Vec<ratatui::text::Line<'static>> {
        use unicode_width::UnicodeWidthChar;
        let mut visual_lines = Vec::new();
        for line in text.lines() {
            if line.is_empty() {
                visual_lines.push(ratatui::text::Line::from(""));
                continue;
            }
            let mut current_line = String::new();
            let mut current_width = 0;
            for c in line.chars() {
                let char_width = c.width().unwrap_or(0);
                if current_width + char_width > max_width {
                    visual_lines.push(ratatui::text::Line::from(current_line.clone()));
                    current_line.clear();
                    current_width = 0;
                }
                current_line.push(c);
                current_width += char_width;
            }
            if !current_line.is_empty() {
                visual_lines.push(ratatui::text::Line::from(current_line));
            }
        }
        visual_lines
    }
}

pub fn format_request_snapshot_to_http(req: &crate::history::model::RequestSnapshot) -> String {
    let mut s = String::new();
    s.push_str(&format!("{} {}\n", req.method, req.url));
    for (k, v) in &req.headers {
        if let Ok(val_str) = v.to_str() {
            s.push_str(&format!("{}: {}\n", k.as_str(), val_str));
        }
    }
    s.push('\n');
    if let Some(ref body) = req.body {
        s.push_str(body);
    }
    s
}

/// 提取 URL 中的 Host 和 Path 组合，剥离协议前缀（如 http://, https://）
/// 例如："https://baidu.com/api/v1?q=1" -> "baidu.com/api/v1"
/// 如果解析失败，则做降级的前缀剥离。
pub fn format_url_host_and_path(url_str: &str) -> String {
    if let Ok(u) = url::Url::parse(url_str) {
        let scheme = u.scheme();
        if scheme == "http" || scheme == "https" || scheme == "ws" || scheme == "wss" {
            let host = u.host_str().unwrap_or("");
            let port_part = if let Some(port) = u.port() {
                format!(":{}", port)
            } else {
                "".to_string()
            };
            let path = u.path();
            let combined = format!("{}{}{}", host, port_part, path);
            if !combined.is_empty() {
                return combined;
            }
        }
    }

    let stripped = url_str
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    if let Some(pos) = stripped.find('?') {
        stripped[..pos].to_string()
    } else if let Some(pos) = stripped.find('#') {
        stripped[..pos].to_string()
    } else {
        stripped.to_string()
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
    use crate::history::model::RequestSnapshot;
    use reqwest::header::HeaderMap;

    #[test]
    fn test_format_url_host_and_path() {
        assert_eq!(
            format_url_host_and_path("https://baidu.com/api/v1?q=1"),
            "baidu.com/api/v1"
        );
        assert_eq!(
            format_url_host_and_path("http://localhost:8080/users/1#fragment"),
            "localhost:8080/users/1"
        );
        assert_eq!(format_url_host_and_path("http://google.com"), "google.com/");
        assert_eq!(
            format_url_host_and_path("localhost:3000/test?foo=bar"),
            "localhost:3000/test"
        );
        assert_eq!(format_url_host_and_path("/relative/path"), "/relative/path");
    }

    #[test]
    fn test_visual_line_wrap_english() {
        let text = "abcdefghij\nklmnopqrst";
        let lines = VisualLineProcessor::wrap_text(text, 5);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].to_string(), "abcde");
        assert_eq!(lines[1].to_string(), "fghij");
        assert_eq!(lines[2].to_string(), "klmno");
        assert_eq!(lines[3].to_string(), "pqrst");
    }

    #[test]
    fn test_visual_line_wrap_chinese_multibyte() {
        let text = "你好世界，你好。"; // 8 个字，逻辑宽度 16
        let lines = VisualLineProcessor::wrap_text(text, 6);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].to_string(), "你好世");
        assert_eq!(lines[1].to_string(), "界，你");
        assert_eq!(lines[2].to_string(), "好。");
    }

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

    #[test]
    fn test_history_restore_format() {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", "application/json".parse().unwrap());
        headers.insert("x-custom", "value".parse().unwrap());

        let req = RequestSnapshot {
            method: "POST".to_string(),
            url: "http://example.com/api".to_string(),
            headers,
            body: Some("{\"test\":true}".to_string()),
        };

        let http_text = format_request_snapshot_to_http(&req);
        let parsed = crate::parser::parse_content(&http_text).unwrap();
        let requests = parsed.requests;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method_or_default(), "POST");
        assert_eq!(requests[0].url, "http://example.com/api");
        assert_eq!(requests[0].headers.len(), 2);
        assert_eq!(requests[0].body, Some("{\"test\":true}".to_string()));
    }
}
