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

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone)]
pub struct AppState {
    pub active_panel: Panel,
    pub is_loading: bool,
    pub show_help: bool,
    pub layout_mode: LayoutMode,
    pub terminal_width: u16,
    pub terminal_height: u16,

    // 数据模型
    pub file_tree: Vec<String>, // 扁平文件树列表数据
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
            file_tree: Vec::new(),
            selected_file_index: 0,
            current_request: None,
            last_response: None,
            assertions: Vec::new(),
            variables: VariableContext::new(),
            quick_input: None,
            is_quitting: false,
            editor_text: String::new(),
            editor_file_path: None,
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
                self.sse_stream_body.clear();
                self.ws_frames.clear();
                self.response_scroll = 0;
                self.response_visual_lines.clear();
            }
            Action::UpdateQuickInput(val) => {
                self.quick_input = Some(val);
            }
        }
    }

    pub fn handle_stream_chunk(&mut self, chunk: String) {
        self.total_log_lines += chunk.matches('\n').count();
        self.sse_stream_body.push_str(&chunk);
        if self.active_panel == Panel::Response {
            self.response_scroll = 9999;
        }
    }

    pub fn handle_ws_frame(&mut self, is_send: bool, content: String) {
        self.total_log_lines += 1;
        let now = chrono::Local::now().format("%H:%M:%S").to_string();
        self.ws_frames.push(WsFrameRecord {
            is_send,
            timestamp: now,
            content,
        });
        if self.ws_frames.len() > 100 {
            self.ws_frames.remove(0);
        }
        if self.active_panel == Panel::Response {
            self.response_scroll = 9999;
        }
    }

    /// 处理完成的请求结果同步变量
    pub fn handle_request_finished(
        &mut self,
        result: Result<Response, String>,
        captured_vars: HashMap<String, String>,
        assertions: Vec<AssertionResult>,
    ) {
        self.is_loading = false;
        self.assertions = assertions;
        match result {
            Ok(resp) => {
                self.last_response = Some(resp);
                // 同步变量到全局上下文，防止多线程借用争用
                self.variables.extend(captured_vars);
            }
            Err(e) => {
                self.last_response = Some(Response::error(e));
            }
        }
    }

    /// 滚动视口滑动窗口加载算法 (Sliding Viewport)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::model::RequestSnapshot;
    use reqwest::header::HeaderMap;

    #[test]
    fn test_format_url_host_and_path() {
        assert_eq!(format_url_host_and_path("https://baidu.com/api/v1?q=1"), "baidu.com/api/v1");
        assert_eq!(format_url_host_and_path("http://localhost:8080/users/1#fragment"), "localhost:8080/users/1");
        assert_eq!(format_url_host_and_path("http://google.com"), "google.com/");
        assert_eq!(format_url_host_and_path("localhost:3000/test?foo=bar"), "localhost:3000/test");
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
