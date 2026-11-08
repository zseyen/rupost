#![allow(clippy::collapsible_if)]
use super::ports::Repo;
use super::theme::{Theme, ThemeKind, palette};

mod creation;
mod history;
mod layout;
mod lifecycle;
mod retention;
pub mod search;
mod stream;
#[cfg(test)]
mod tests;
mod tree;
mod wrap;

use crate::assertion::AssertionResult;
use crate::http::Response;
use crate::parser::ParsedRequest;
use crate::variable::VariableContext;
pub use history::{format_request_snapshot_to_http, format_url_host_and_path};
pub use layout::LayoutMode;
pub use retention::{cleanup_old_logs_dir, write_log_with_limit};
use std::path::PathBuf;
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
    /// 多请求执行结果摘要（run_results）
    Results,
    /// 失败自动诊断数据（last_diagnose）
    Diagnose,
}

use std::collections::HashSet;

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
    /// 全局 `A` 快速授权（输入 header 值，Enter 存入 `__quick_auth` 变量）
    Auth(Box<ratatui_textarea::TextArea<'static>>),
    /// Response 面板 `/` 消息搜索（实时匹配，Esc/Enter 退出后 n/N 续跳）
    Search(Box<ratatui_textarea::TextArea<'static>>),
    /// WS 交互会话发送（`S` 键，Enter 发送）
    WsSend(Box<ratatui_textarea::TextArea<'static>>),
    /// 单请求选择器（`#` 键，`#3` / `@name`）
    Selector(Box<ratatui_textarea::TextArea<'static>>),
    /// AI 断言翻译（`I` 键，Enter 提交自然语言）
    AiAssert(Box<ratatui_textarea::TextArea<'static>>),
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

    // 消息搜索状态（R4：Response 面板 `/`；query 留存供 n/N 续跳）
    pub search_query: String,
    pub search_match_index: usize,

    // WS 交互会话（R6/R7）：Some 时活跃，drop 即关闭；pending 发送由模态取出
    pub ws_session: Option<crate::tui::ws_interactive::WsInteractiveSession>,
    pub pending_ws_send: Option<String>,

    // CLI 能力接入（R2-R9）：执行模型 / 调试增强 / 快照
    pub run_results: Vec<crate::runner::types::TestResult>,
    pub run_total: usize,
    pub run_index: usize,
    pub run_selector: Option<String>,
    pub cookie_bridge: std::sync::Arc<std::sync::Mutex<Option<serde_json::Value>>>,
    pub cookie_file_path: Option<String>,
    pub mock_session: Option<super::integrations::MockSessionState>,
    pub last_replay: Option<crate::runner::ReplayReport>,
    pub pending_ai_text: Option<String>,
    pub last_timing: Option<crate::http::timing::RequestTiming>,
    pub last_diagnose: Option<crate::http::diagnose::DiagnosticsReport>,
    /// X 键导出的命令文本（弹窗展示，Esc 关闭）
    pub show_export: Option<String>,

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
            search_query: String::new(),
            search_match_index: 0,
            ws_session: None,
            pending_ws_send: None,
            run_results: Vec::new(),
            run_total: 0,
            run_index: 0,
            run_selector: None,
            cookie_bridge: std::sync::Arc::new(std::sync::Mutex::new(None)),
            cookie_file_path: None,
            mock_session: None,
            last_replay: None,
            pending_ai_text: None,
            last_timing: None,
            last_diagnose: None,
            show_export: None,

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

    /// 快速授权变量键（`A` 键设置；空串 = 未设置）
    pub const QUICK_AUTH_VAR: &'static str = "__quick_auth";

    /// 设置快速授权（Authorization header 值，如 "Bearer eyJ..."）
    pub fn set_quick_auth(&mut self, value: &str) {
        self.variables.insert(Self::QUICK_AUTH_VAR, value);
    }

    /// 清除快速授权
    pub fn clear_quick_auth(&mut self) {
        self.variables.insert(Self::QUICK_AUTH_VAR, "");
    }

    /// 当前快速授权值（未设置或空返回 None）
    pub fn quick_auth(&self) -> Option<String> {
        self.variables
            .get(Self::QUICK_AUTH_VAR)
            .filter(|v| !v.is_empty())
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
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
